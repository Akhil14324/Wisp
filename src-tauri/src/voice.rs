//! Hold-to-talk recording shared by dictation, commands and Circle to Ask.

use serde_json::json;
use tokio::sync::mpsc;

use crate::audio::{AudioError, AudioEvent, Recording};
use crate::hotkeys::ChordAction;
use crate::island::State;
use crate::state::ctx;

/// Starts recording for `action` (FR-DIC-01). Returns false on failure.
pub async fn start(action: ChordAction) -> bool {
    let c = ctx();
    {
        let mut g = c.recording_for.lock().unwrap_or_else(|p| p.into_inner());
        if g.is_some() {
            return false;
        }
        *g = Some(action);
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<AudioEvent>();
    let device = c.settings().audio.input_device;
    match c.recorder.start(device, tx).await {
        Ok(()) => {
            c.island.set(State::Listening, None);
            tauri::async_runtime::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    match ev {
                        AudioEvent::Level(rms) => c.emit("audio-level", json!({ "rms": rms })),
                        AudioEvent::Full => {
                            // FR-DIC-02: stop automatically at 120 s, as if released.
                            let pending =
                                *c.recording_for.lock().unwrap_or_else(|p| p.into_inner());
                            if let Some(a) = pending {
                                tracing::info!("recording hit 120 s limit");
                                crate::dispatch::chord_up(a).await;
                            }
                        }
                        AudioEvent::DeviceLost => {
                            let pending = c
                                .recording_for
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .take();
                            if pending.is_some() {
                                let _ = c.recorder.stop().await;
                                crate::cta::cancel_drawing();
                                c.island.error_text("The microphone was disconnected");
                            }
                        }
                    }
                }
            });
            true
        }
        Err(e) => {
            *c.recording_for.lock().unwrap_or_else(|p| p.into_inner()) = None;
            tracing::warn!(error = %e, "recording failed to start");
            c.island.error_text(&e.to_string());
            false
        }
    }
}

/// Stops recording if it belongs to `action`.
pub async fn stop(action: ChordAction) -> Option<Result<Recording, AudioError>> {
    let c = ctx();
    {
        let mut g = c.recording_for.lock().unwrap_or_else(|p| p.into_inner());
        if *g != Some(action) {
            return None;
        }
        *g = None;
    }
    Some(c.recorder.stop().await)
}
