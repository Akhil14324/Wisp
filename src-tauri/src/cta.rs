//! Circle to Ask (section 4.5) plus the shared overlay window used for
//! Computer Use target rings (FR-CU-11).

use std::sync::Mutex;
use std::time::Duration;

use image::RgbaImage;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

use crate::capture::{self, MonitorInfo, Stroke};
use crate::error::{ErrCode, UiError, UiResult};
use crate::gemini::{self, client, prompts};
use crate::hotkeys::ChordAction;
use crate::island::State;
use crate::state::ctx;
use crate::winutil;

pub const OVERLAY: &str = "overlay";
pub const CARD: &str = "card";
const CARD_W: f64 = 360.0;
const MAX_TURNS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Drawing,
    FollowUpVoice,
    Rings,
}

struct Session {
    contents: Vec<Value>,
    images: Vec<Value>,
    has_strokes: bool,
    turns: u32,
}

#[derive(Default)]
pub struct CtaState {
    mode: Mutex<Option<Mode>>,
    shot: Mutex<Option<(MonitorInfo, RgbaImage)>>,
    strokes_tx: Mutex<Option<oneshot::Sender<Vec<Stroke>>>>,
    session: Mutex<Option<Session>>,
    /// Card rectangle in physical pixels, for follow-up by voice.
    card_rect: Mutex<Option<(i32, i32, i32, i32)>>,
}

#[derive(Debug, Clone, Serialize)]
struct CardUpdate {
    /// loading | answer | error | ask
    state: &'static str,
    text: String,
    can_follow_up: bool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Creates the hidden overlay and card windows at startup.
pub fn create_windows(app: &tauri::AppHandle) -> tauri::Result<()> {
    let ov = WebviewWindowBuilder::new(
        app,
        OVERLAY,
        WebviewUrl::App("src/overlay/index.html".into()),
    )
    .title("Wisp overlay")
    .transparent(true)
    .decorations(false)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(false)
    .build()?;
    if let Ok(h) = ov.hwnd() {
        winutil::exclude_from_capture(h.0 as isize);
    }
    let card = WebviewWindowBuilder::new(app, CARD, WebviewUrl::App("src/card/index.html".into()))
        .title("Wisp answer")
        .inner_size(CARD_W, 160.0)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .build()?;
    if let Ok(h) = card.hwnd() {
        winutil::exclude_from_capture(h.0 as isize);
    }
    Ok(())
}

fn overlay() -> Option<tauri::WebviewWindow> {
    ctx().app.get_webview_window(OVERLAY)
}

fn card() -> Option<tauri::WebviewWindow> {
    ctx().app.get_webview_window(CARD)
}

fn place_overlay(w: &tauri::WebviewWindow, m: &MonitorInfo) {
    let _ = w.set_position(PhysicalPosition::new(m.x, m.y));
    let _ = w.set_size(PhysicalSize::new(m.width, m.height));
    let _ = w.set_position(PhysicalPosition::new(m.x, m.y));
}

fn card_visible() -> bool {
    card().and_then(|w| w.is_visible().ok()).unwrap_or(false)
}

/// Chord pressed (FR-CTA-01): capture first, then show the overlay.
pub async fn begin() {
    let c = ctx();
    if lock(&c.cta.mode).is_some_and(|m| m != Mode::Rings) {
        return;
    }
    let (cx, cy) = capture::cursor_pos();

    // FR-CTA-07: holding the chord over the card is a spoken follow-up.
    let over_card = card_visible()
        && lock(&c.cta.card_rect)
            .map(|(x, y, w, h)| cx >= x && cy >= y && cx < x + w && cy < y + h)
            .unwrap_or(false)
        && lock(&c.cta.session).is_some();
    if over_card {
        *lock(&c.cta.mode) = Some(Mode::FollowUpVoice);
        if !crate::voice::start(ChordAction::Circle).await {
            *lock(&c.cta.mode) = None;
        }
        return;
    }

    let Some(m) = capture::monitor_at(cx, cy) else {
        return;
    };
    let m2 = m.clone();
    let shot = tokio::task::spawn_blocking(move || capture::capture(&m2)).await;
    let img = match shot {
        Ok(Ok(img)) => img,
        _ => {
            c.island.error_text("Screen capture failed");
            return;
        }
    };
    *lock(&c.cta.shot) = Some((m.clone(), img));
    *lock(&c.cta.mode) = Some(Mode::Drawing);
    if let Some(w) = overlay() {
        let _ = w.set_ignore_cursor_events(false);
        place_overlay(&w, &m);
        c.emit("overlay-init", json!({ "monitor": m.id, "width": m.width, "height": m.height, "scale": m.scale, "mode": "draw" }));
        let _ = w.show();
        let _ = w.set_focus();
    }
    if !crate::voice::start(ChordAction::Circle).await {
        cancel_drawing();
    }
}

/// Called by the overlay with its strokes (physical, monitor-local pixels).
pub fn submit_strokes(strokes: Vec<Stroke>) {
    if let Some(tx) = lock(&ctx().cta.strokes_tx).take() {
        let _ = tx.send(strokes);
    }
}

/// Aborts drawing without asking (mic error).
pub fn cancel_drawing() {
    let c = ctx();
    let mut mode = lock(&c.cta.mode);
    if *mode == Some(Mode::Drawing) || *mode == Some(Mode::FollowUpVoice) {
        *mode = None;
        lock(&c.cta.shot).take();
        if let Some(w) = overlay() {
            let _ = w.hide();
        }
    }
}

/// Chord released (FR-CTA-04).
pub async fn finish() {
    let c = ctx();
    let mode = lock(&c.cta.mode).take();
    match mode {
        Some(Mode::FollowUpVoice) => {
            let rec = crate::voice::stop(ChordAction::Circle).await;
            c.island.idle();
            if let Some(Ok(r)) = rec {
                if !r.too_short() {
                    let part = gemini::inline_part("audio/wav", &r.wav());
                    tauri::async_runtime::spawn(async move {
                        let _ = follow_up(part).await;
                    });
                }
            }
        }
        Some(Mode::Drawing) => {
            let (tx, rx) = oneshot::channel();
            *lock(&c.cta.strokes_tx) = Some(tx);
            c.emit("overlay-finish", json!({}));
            let strokes = tokio::time::timeout(Duration::from_millis(700), rx)
                .await
                .ok()
                .and_then(|r| r.ok())
                .unwrap_or_default();
            if let Some(w) = overlay() {
                let _ = w.hide();
            }
            let rec = crate::voice::stop(ChordAction::Circle).await;
            let Some((monitor, shot)) = lock(&c.cta.shot).take() else {
                c.island.idle();
                return;
            };
            let audio = match rec {
                Some(Ok(r)) if !r.too_short() => Some(r.wav()),
                Some(Err(e)) => {
                    c.island.error_text(&e.to_string());
                    return;
                }
                _ => None,
            };
            let strokes: Vec<Stroke> = strokes
                .into_iter()
                .filter(|s| !s.points.is_empty())
                .take(50)
                .collect();
            // The slow part runs on its own task so hotkey events keep flowing.
            tauri::async_runtime::spawn(answer_circle(monitor, shot, strokes, audio));
        }
        _ => {}
    }
}

async fn answer_circle(
    monitor: MonitorInfo,
    shot: RgbaImage,
    strokes: Vec<Stroke>,
    audio: Option<Vec<u8>>,
) {
    let c = ctx();
    let has_strokes = !strokes.is_empty();
    let payload =
        match tokio::task::spawn_blocking(move || capture::circle_payload(&shot, &strokes)).await {
            Ok(Ok(p)) => p,
            _ => {
                c.island.error_text("Could not prepare the screenshot");
                return;
            }
        };
    let mut images = vec![gemini::inline_part("image/jpeg", &payload.context_jpeg)];
    if let Some(crop) = &payload.crop_png {
        images.push(gemini::inline_part("image/png", crop));
    }
    *lock(&c.cta.session) = Some(Session {
        contents: vec![],
        images,
        has_strokes,
        turns: 0,
    });
    show_card(&monitor, payload.crop_rect);
    match audio {
        Some(wav) => {
            let _ = ask(gemini::inline_part("audio/wav", &wav)).await;
        }
        None => {
            // FR-CTA-05: no audio, so let the user type the question.
            c.island.idle();
            update_card("ask", "What do you want to know about this?", false);
        }
    }
}

fn update_card(state: &'static str, text: &str, can_follow_up: bool) {
    ctx().emit(
        "card-update",
        CardUpdate {
            state,
            text: text.to_string(),
            can_follow_up,
        },
    );
}

/// Places the card next to the drawn region, or centred (FR-CTA-06).
fn show_card(m: &MonitorInfo, rect: Option<(u32, u32, u32, u32)>) {
    let Some(w) = card() else { return };
    let scale = m.scale.max(0.5) as f64;
    let cw = (CARD_W * scale).round() as i32;
    let ch = (200.0 * scale).round() as i32;
    let gap = (16.0 * scale) as i32;
    let (mx, my, mw, mh) = (m.x, m.y, m.width as i32, m.height as i32);
    let (x, y) = match rect {
        Some((rx, ry, rw, rh)) => {
            let (rx, ry, rw, rh) = (mx + rx as i32, my + ry as i32, rw as i32, rh as i32);
            let x = if rx + rw + gap + cw <= mx + mw {
                rx + rw + gap
            } else if rx - gap - cw >= mx {
                rx - gap - cw
            } else {
                (rx + rw / 2 - cw / 2).clamp(mx, mx + mw - cw)
            };
            let y = if x >= rx + rw || x + cw <= rx {
                ry.clamp(my + gap, my + mh - ch - gap)
            } else if ry + rh + gap + ch <= my + mh {
                ry + rh + gap
            } else {
                (ry - gap - ch).max(my + gap)
            };
            (x, y)
        }
        None => (mx + (mw - cw) / 2, my + (mh - ch) / 2),
    };
    *lock(&ctx().cta.card_rect) = Some((x, y, cw, ch));
    let _ = w.set_position(PhysicalPosition::new(x, y));
    let _ = w.set_size(tauri::LogicalSize::new(CARD_W, 200.0));
    update_card("loading", "", false);
    let _ = w.show();
    let _ = w.set_focus();
}

/// Card content height changed (front end measures).
pub fn resize_card(height: f64) {
    if let Some(w) = card() {
        let h = height.clamp(96.0, 420.0);
        let _ = w.set_size(tauri::LogicalSize::new(CARD_W, h));
        if let (Some((x, y, cw, _)), Ok(sf)) = (*lock(&ctx().cta.card_rect), w.scale_factor()) {
            *lock(&ctx().cta.card_rect) = Some((x, y, cw, (h * sf).round() as i32));
        }
    }
}

pub fn close_card() {
    let c = ctx();
    lock(&c.cta.session).take();
    lock(&c.cta.card_rect).take();
    if let Some(w) = card() {
        let _ = w.hide();
    }
}

async fn request(question: Value) -> UiResult<String> {
    let c = ctx();
    let key = c.key().ok_or_else(|| UiError::of(ErrCode::NoKey))?;
    let (system, mut contents) = {
        let mut g = lock(&c.cta.session);
        let s = g
            .as_mut()
            .ok_or_else(|| UiError::invalid("No question is open"))?;
        if s.turns >= MAX_TURNS {
            return Err(UiError::invalid(
                "Follow-ups are limited to 5 turns. Start a new circle.",
            ));
        }
        let base = if s.has_strokes {
            prompts::CIRCLE_TO_ASK
        } else {
            prompts::CIRCLE_TO_ASK_NO_STROKES
        };
        let system = prompts::with_soul(base, &c.soul().addendum);
        let parts = if s.contents.is_empty() {
            let mut p = s.images.clone();
            p.push(question);
            p
        } else {
            vec![question]
        };
        s.contents.push(json!({ "role": "user", "parts": parts }));
        (system, s.contents.clone())
    };
    let body =
        json!({ "systemInstruction": gemini::system_instruction(&system), "contents": contents });
    let model = c.settings().models.vision;
    let resp = c
        .gemini
        .generate_content(
            &key,
            &model,
            &body,
            client::TIMEOUT,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await;
    let mut g = lock(&c.cta.session);
    let Some(s) = g.as_mut() else {
        return Err(UiError::of(ErrCode::Cancelled));
    };
    match resp {
        Ok(v) => {
            let text = gemini::response_text(&v);
            if text.trim().is_empty() {
                s.contents.pop();
                return Err(UiError::of(ErrCode::EmptyResult));
            }
            if let Some(mc) = gemini::model_content(&v) {
                s.contents.push(mc);
            }
            s.turns += 1;
            Ok(text.trim().to_string())
        }
        Err(e) => {
            s.contents.pop();
            contents.clear();
            Err(e.into())
        }
    }
}

async fn ask(question: Value) -> UiResult<String> {
    let c = ctx();
    c.island.set(State::Thinking, Some("Thinking"));
    update_card("loading", "", false);
    let r = request(question).await;
    c.island.idle();
    match &r {
        Ok(t) => {
            let more = lock(&c.cta.session)
                .as_ref()
                .map(|s| s.turns < MAX_TURNS)
                .unwrap_or(false);
            update_card("answer", t, more);
        }
        Err(e) => update_card("error", &e.message, true),
    }
    r
}

/// Typed question (no audio) or typed follow-up (FR-CTA-05/07).
pub async fn ask_text(text: &str) -> UiResult<String> {
    let t: String = text.trim().chars().take(2000).collect();
    if t.is_empty() {
        return Err(UiError::invalid("Type a question first"));
    }
    let first = lock(&ctx().cta.session)
        .as_ref()
        .map(|s| s.contents.is_empty())
        .unwrap_or(false);
    if first {
        ask(gemini::text_part(&t)).await
    } else {
        follow_up(gemini::text_part(&t)).await
    }
}

async fn follow_up(part: Value) -> UiResult<String> {
    ask(part).await
}

/// Shows a 400 ms target ring at a physical screen point (FR-CU-11).
pub fn show_ring(m: &MonitorInfo, x: i32, y: i32) {
    let c = ctx();
    {
        let mut mode = lock(&c.cta.mode);
        if mode.is_some_and(|m| m != Mode::Rings) {
            return;
        }
        if mode.is_none() {
            *mode = Some(Mode::Rings);
            if let Some(w) = overlay() {
                let _ = w.set_ignore_cursor_events(true);
                place_overlay(&w, m);
                c.emit("overlay-init", json!({ "monitor": m.id, "width": m.width, "height": m.height, "scale": m.scale, "mode": "rings" }));
                // Never take focus from the app the agent is operating.
                if let Ok(h) = w.hwnd() {
                    winutil::show_no_activate(h.0 as isize);
                }
            }
        }
    }
    c.emit("overlay-ring", json!({ "x": x - m.x, "y": y - m.y }));
}

pub fn hide_rings() {
    let Some(c) = crate::state::try_ctx() else {
        return;
    };
    let mut mode = lock(&c.cta.mode);
    if *mode == Some(Mode::Rings) {
        *mode = None;
        if let Some(w) = overlay() {
            let _ = w.hide();
            let _ = w.set_ignore_cursor_events(false);
        }
    }
}

/// describe_screen tool: capture under the cursor and ask the vision model.
pub async fn describe_screen(question: &str) -> Result<String, String> {
    let c = ctx();
    let key = c.key().ok_or("No API key")?;
    let (x, y) = capture::cursor_pos();
    let m = capture::monitor_at(x, y).ok_or("No monitor found")?;
    let jpeg = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let img = capture::capture(&m)?;
        capture::to_jpeg(&capture::fit_long_edge(&img, capture::MAX_EDGE), 85)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let body = json!({
        "systemInstruction": gemini::system_instruction(prompts::DESCRIBE_SCREEN),
        "contents": [{ "role": "user", "parts": [gemini::inline_part("image/jpeg", &jpeg), gemini::text_part(question)] }]
    });
    let model = c.settings().models.vision;
    let v = c
        .gemini
        .generate_content(
            &key,
            &model,
            &body,
            client::TIMEOUT,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .map_err(|e| e.code().island_message().to_string())?;
    let t = gemini::response_text(&v);
    if t.trim().is_empty() {
        Err("Gemini returned no answer".into())
    } else {
        Ok(t.trim().chars().take(2000).collect())
    }
}
