//! Dictation (section 4.3) and the shared text-insertion path (FR-DIC-06/07).

use std::time::{Duration, Instant};

use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::audio::Recording;
use crate::clipboard::{self, Contents};
use crate::error::ErrCode;
use crate::gemini::{self, client, prompts};
use crate::input;
use crate::island::State;
use crate::state::ctx;
use crate::store::history;
use crate::winutil;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inserted {
    Pasted,
    Typed,
    /// Foreground is elevated: text left on the clipboard instead.
    ClipboardOnly,
}

/// Inserts text into the focused window. Blocking; run on a blocking thread.
pub fn insert_text(text: &str) -> anyhow::Result<Inserted> {
    // Let go of the chord first, or Ctrl+V would become Win+V, etc.
    winutil::wait_modifiers_released(Duration::from_millis(1500));
    let fg = winutil::foreground();
    if fg.elevated {
        clipboard::write_text(text)?;
        crate::notify::show(
            "Text copied instead",
            "Windows blocks typing into apps running as administrator. Your text is on the clipboard; press Ctrl+V.",
        );
        return Ok(Inserted::ClipboardOnly);
    }
    match clipboard::contents() {
        Ok(Contents::Text(old)) => paste_and_restore(text, Some(old)),
        Ok(Contents::Empty) => paste_and_restore(text, None),
        // Non-text data on the clipboard: type instead so it is never lost (section 7.5).
        Ok(Contents::NonText) | Err(_) => {
            input::type_text(text, None)?;
            Ok(Inserted::Typed)
        }
    }
}

fn paste_and_restore(text: &str, old: Option<String>) -> anyhow::Result<Inserted> {
    if clipboard::write_text(text).is_err() {
        input::type_text(text, None)?;
        return Ok(Inserted::Typed);
    }
    let pasted = input::press_keys("ctrl+v");
    std::thread::sleep(Duration::from_millis(300));
    match old {
        Some(o) => {
            let _ = clipboard::write_text(&o);
        }
        None => {
            let _ = clipboard::clear();
        }
    }
    if pasted.is_err() {
        // FR-DIC-07: the target refused the paste; type it instead.
        input::type_text(text, None)?;
        return Ok(Inserted::Typed);
    }
    Ok(Inserted::Pasted)
}

/// Sends the recording to the dictation model and inserts the cleaned text.
pub async fn finish(rec: Recording) {
    let c = ctx();
    if rec.too_short() {
        // FR-DIC-02: under 300 ms is discarded silently.
        c.island.idle();
        return;
    }
    let Some(key) = c.key() else {
        c.island.error(ErrCode::NoKey);
        return;
    };
    let started = Instant::now();
    c.island.set(State::Thinking, Some("Thinking"));
    let settings = c.settings();
    let body = json!({
        "systemInstruction": gemini::system_instruction(&prompts::dictation(&settings.dictation.dictionary)),
        "contents": [{ "role": "user", "parts": [gemini::inline_part("audio/wav", &rec.wav())] }]
    });
    drop(rec);
    let resp = c
        .gemini
        .generate_content(
            &key,
            &settings.models.dictation,
            &body,
            client::TIMEOUT,
            &CancellationToken::new(),
        )
        .await;
    let text = match resp {
        Ok(v) => prompts::clean_dictation(&gemini::response_text(&v)),
        Err(e) => {
            tracing::info!(code = e.code().as_str(), "dictation failed");
            c.island.error(e.code());
            return;
        }
    };
    let Some(text) = text else {
        c.island.error(ErrCode::EmptyResult);
        return;
    };
    let t2 = text.clone();
    let res = tokio::task::spawn_blocking(move || insert_text(&t2)).await;
    match res {
        Ok(Ok(how)) => {
            tracing::info!(how = ?how, ms = started.elapsed().as_millis() as u64, "dictation inserted");
            c.island.idle();
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "insertion failed");
            c.island.error_text("Could not insert the text");
        }
        Err(_) => c.island.error(ErrCode::Internal),
    }
    if settings.dictation.history_enabled {
        let _ = history::add_dictation(&c.db, &text);
        c.emit("history-changed", json!({}));
    }
}
