//! Routes hotkey and system events to features.
//!
//! Hook events are handled in order (a release must never overtake its press),
//! so each handler does only the quick part (start or stop recording, show or
//! hide the overlay) and spawns the slow part (Gemini requests, tool runs).

use std::sync::atomic::Ordering;

use crate::agent::{self, CommandInput, RunOpts, Source};
use crate::hotkeys::{ChordAction, HookEvent};
use crate::state::ctx;
use crate::sysevents::SysEvent;

pub async fn on_hook(e: HookEvent) {
    match e {
        HookEvent::ChordDown(a) => chord_down(a).await,
        HookEvent::ChordUp(a) => chord_up(a).await,
        HookEvent::EmergencyStop => {
            let c = ctx();
            if c.agent.is_running()
                || c.agent.has_pending_confirm()
                || c.agent.cu_running.load(Ordering::SeqCst)
            {
                c.agent.stop();
                c.island.result("Stopped");
            }
        }
        HookEvent::PhysicalInput => {
            let c = ctx();
            if c.agent.cu_running.load(Ordering::SeqCst)
                && !c.agent.is_paused()
                && !c.agent.has_pending_confirm()
            {
                c.agent.pause();
                c.emit("agent-paused", serde_json::json!({ "paused": true }));
                c.island
                    .set(crate::island::State::Confirm, Some("Paused: you took over"));
            }
        }
    }
}

async fn chord_down(a: ChordAction) {
    let c = ctx();
    match a {
        ChordAction::Dictation | ChordAction::Command => {
            if a == ChordAction::Command && c.agent.is_running() {
                return;
            }
            crate::voice::start(a).await;
        }
        ChordAction::Circle => crate::cta::begin().await,
    }
}

pub async fn chord_up(a: ChordAction) {
    let c = ctx();
    match a {
        ChordAction::Dictation => match crate::voice::stop(a).await {
            Some(Ok(rec)) => {
                tauri::async_runtime::spawn(crate::dictation::finish(rec));
            }
            Some(Err(e)) => c.island.error_text(&e.to_string()),
            None => {}
        },
        ChordAction::Command => match crate::voice::stop(a).await {
            Some(Ok(rec)) => {
                if rec.too_short() {
                    c.island.idle();
                    return;
                }
                let wav = rec.wav();
                drop(rec);
                tauri::async_runtime::spawn(async move {
                    let _ = agent::run_command(
                        CommandInput::Audio(wav),
                        RunOpts {
                            source: Source::Voice,
                            allowed: ctx().loadout(),
                            unattended: false,
                        },
                    )
                    .await;
                });
            }
            Some(Err(e)) => c.island.error_text(&e.to_string()),
            None => {}
        },
        ChordAction::Circle => crate::cta::finish().await,
    }
}

pub async fn on_sys(e: SysEvent) {
    let c = ctx();
    match e {
        SysEvent::Locked => c.session_locked.store(true, Ordering::SeqCst),
        SysEvent::Unlocked => {
            c.session_locked.store(false, Ordering::SeqCst);
            c.scheduler.wake();
        }
        SysEvent::Resumed => {
            // NFR-32: reinstall the hook and refresh devices after sleep.
            crate::hotkeys::reinstall();
            c.scheduler.wake();
            c.emit("devices-changed", serde_json::json!({}));
        }
        SysEvent::ClipboardChanged => {
            if crate::clipboard::SELF_WRITE.swap(false, Ordering::SeqCst) {
                return;
            }
            if !c.settings().clipboard.history_enabled {
                return;
            }
            let item = tokio::task::spawn_blocking(|| {
                if crate::clipboard::excluded_from_history() {
                    return None;
                }
                crate::clipboard::read_text().ok().flatten()
            })
            .await
            .ok()
            .flatten();
            if let Some(t) = item {
                c.clip_history.push(t);
                c.emit("clipboard-changed", serde_json::json!({}));
            }
        }
    }
}
