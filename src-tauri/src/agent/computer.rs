//! Computer Use loop with its safety gate (section 4.7, 5.7).

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::ConfirmKind;
use crate::capture::{self, MonitorInfo};
use crate::error::ErrCode;
use crate::gemini::client::TIMEOUT_COMPUTER_USE;
use crate::gemini::computer_use::{self as cu, ActionResult, CuAction};
use crate::input::{self, keys, Button};
use crate::island::State;
use crate::state::ctx;
use crate::store::history;
use crate::store::settings::GuardList;
use crate::winutil::{self, Foreground};

const STUCK_REPEATS: usize = 3;

/// Independent guard (FR-CU-08): process name or title keyword match.
pub fn guard_hit(fg: &Foreground, guard: &GuardList) -> Option<String> {
    let proc_lower = fg.process.to_ascii_lowercase();
    if let Some(p) = guard
        .processes
        .iter()
        .find(|p| p.to_ascii_lowercase() == proc_lower && !proc_lower.is_empty())
    {
        return Some(format!("{p} is in front"));
    }
    let title = fg.title.to_lowercase();
    guard
        .title_keywords
        .iter()
        .find(|k| !k.trim().is_empty() && title.contains(&k.to_lowercase()))
        .map(|k| format!("the window title mentions \"{k}\""))
}

fn px(m: &MonitorInfo, x: u32, y: u32) -> (i32, i32) {
    (
        cu::to_pixel(x, m.width, m.x),
        cu::to_pixel(y, m.height, m.y),
    )
}

fn single_key(name: &str) -> Result<u16, String> {
    let chord = keys::parse_chord(name).map_err(|e| e.0)?;
    if chord.keys.len() != 1 {
        return Err("key_down and key_up take a single key".into());
    }
    Ok(chord.keys[0])
}

/// Executes one validated action with real input (FR-CU-05). Blocking.
fn execute(m: &MonitorInfo, a: &CuAction, cancel: &CancellationToken) -> Result<(), String> {
    let e = |r: Result<(), input::InputError>| r.map_err(|e| e.to_string());
    match a {
        CuAction::Click { x, y } => {
            let (px_, py) = px(m, *x, *y);
            e(input::click(Button::Left, px_, py, 1))
        }
        CuAction::DoubleClick { x, y } => {
            let (px_, py) = px(m, *x, *y);
            e(input::click(Button::Left, px_, py, 2))
        }
        CuAction::TripleClick { x, y } => {
            let (px_, py) = px(m, *x, *y);
            e(input::click(Button::Left, px_, py, 3))
        }
        CuAction::MiddleClick { x, y } => {
            let (px_, py) = px(m, *x, *y);
            e(input::click(Button::Middle, px_, py, 1))
        }
        CuAction::RightClick { x, y } => {
            let (px_, py) = px(m, *x, *y);
            e(input::click(Button::Right, px_, py, 1))
        }
        CuAction::MouseDown { x, y } => {
            let (px_, py) = px(m, *x, *y);
            e(input::move_to(px_, py)).and(e(input::button(Button::Left, true, px_, py)))
        }
        CuAction::MouseUp { x, y } => {
            let (px_, py) = px(m, *x, *y);
            e(input::button(Button::Left, false, px_, py))
        }
        CuAction::Move { x, y } => {
            let (px_, py) = px(m, *x, *y);
            e(input::move_to(px_, py))
        }
        CuAction::Type { text, press_enter } => {
            e(input::type_text(text, Some(cancel)))?;
            if *press_enter {
                e(input::press_keys("enter"))?;
            }
            Ok(())
        }
        CuAction::DragAndDrop {
            start_x,
            start_y,
            end_x,
            end_y,
        } => e(input::drag(
            px(m, *start_x, *start_y),
            px(m, *end_x, *end_y),
        )),
        CuAction::Wait { seconds } => {
            let until = Instant::now() + Duration::from_secs(*seconds as u64);
            while Instant::now() < until && !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(())
        }
        CuAction::PressKey { key } => {
            let chord = keys::parse_chord(key).map_err(|e| e.0)?;
            if keys::is_blocked(&chord, winutil::foreground().is_wisp) {
                return Err("that key combination is blocked".into());
            }
            e(input::press_chord(&chord))
        }
        CuAction::KeyDown { key } => e(input::key_down(single_key(key)?)),
        CuAction::KeyUp { key } => e(input::key_up(single_key(key)?)),
        CuAction::Hotkey { keys: ks } => {
            let chord = keys::parse_chord(&ks.join("+")).map_err(|e| e.0)?;
            if keys::is_blocked(&chord, winutil::foreground().is_wisp) {
                return Err("that key combination is blocked".into());
            }
            e(input::press_chord(&chord))
        }
        CuAction::TakeScreenshot => Ok(()),
        CuAction::Scroll {
            x,
            y,
            direction,
            magnitude,
        } => {
            let (px_, py) = px(m, *x, *y);
            let notches = ((*magnitude as f32 / 100.0).round() as i32).clamp(1, 10);
            e(input::scroll(px_, py, direction, notches))
        }
    }
}

async fn screenshot(m: &MonitorInfo) -> Result<(image::RgbaImage, &'static str, Vec<u8>), String> {
    let m = m.clone();
    tokio::task::spawn_blocking(move || {
        let img = capture::capture(&m).map_err(|e| format!("screen capture failed: {e}"))?;
        if capture::is_black(&img) {
            return Err("The screen captured as black (lock screen, secure desktop or protected video), so Wisp cannot see it".to_string());
        }
        let (mime, bytes) = capture::encode_for_agent(&img).map_err(|e| e.to_string())?;
        Ok((img, mime, bytes))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn debug_save(enabled: bool, step: usize, bytes: &[u8], mime: &str) {
    if !enabled {
        return;
    }
    let Ok(appdata) = std::env::var("APPDATA") else {
        return;
    };
    let dir = std::path::Path::new(&appdata).join("Wisp").join("debug");
    let _ = std::fs::create_dir_all(&dir);
    let ext = if mime == "image/png" { "png" } else { "jpg" };
    let name = format!(
        "{}-step{step:02}.{ext}",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let _ = std::fs::write(dir.join(name), bytes);
}

/// Deletes debug screenshots older than 24 h (FR-CU-12).
pub fn cleanup_debug() {
    let Ok(appdata) = std::env::var("APPDATA") else {
        return;
    };
    let dir = std::path::Path::new(&appdata).join("Wisp").join("debug");
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().unwrap_or_default() > Duration::from_secs(24 * 3600))
            .unwrap_or(false);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

struct Cleanup;

impl Drop for Cleanup {
    fn drop(&mut self) {
        let c = ctx();
        c.agent.cu_running.store(false, Ordering::SeqCst);
        crate::hotkeys::set_report_physical(false);
        input::release_all();
        crate::cta::hide_rings();
        c.island.set_controlling(false);
        c.agent.resume();
    }
}

/// Runs a Computer Use task. Returns the model's summary or a failure reason.
pub async fn run_task(
    goal: &str,
    cancel: &CancellationToken,
    unattended: bool,
) -> Result<String, String> {
    let c = ctx();
    let key = c
        .key()
        .ok_or_else(|| ErrCode::NoKey.island_message().to_string())?;
    let settings = c.settings();
    let model = settings.models.computer_use.clone();
    let confirm_timeout = if unattended {
        Duration::from_secs(600)
    } else {
        Duration::from_secs(300)
    };
    let goal: String = goal.trim().chars().take(1000).collect();
    if goal.is_empty() {
        return Err("The task has no goal".into());
    }

    // FR-CU-02: first task per app session needs a yes.
    if !c.agent.cu_session_ok.load(Ordering::SeqCst) {
        if unattended {
            crate::notify::show(
                "Wisp needs your OK",
                "A scheduled task wants to control your PC. Open Wisp to answer.",
            );
        }
        let ok = c
            .agent
            .confirm(
                ConfirmKind::FirstTask,
                "Let Wisp control your PC?",
                &format!("Goal: {goal}\nWisp will use your mouse and keyboard. Press Esc three times to stop."),
                confirm_timeout,
                cancel,
            )
            .await;
        if !ok {
            return Err("You declined the task".into());
        }
        c.agent.cu_session_ok.store(true, Ordering::SeqCst);
    }

    let monitor =
        capture::monitor_for_setting(&settings.capture.target_monitor).ok_or("No monitor found")?;
    let history_id =
        history::add_command(&c.db, "agent_task", Some(&goal), &json!([]), "running", 0).ok();
    let started = Instant::now();
    let deadline =
        started + Duration::from_secs(settings.agent.max_minutes.clamp(1, 10) as u64 * 60);
    let max_steps = settings.agent.max_steps.clamp(1, 40) as usize;

    c.agent.cu_running.store(true, Ordering::SeqCst);
    crate::hotkeys::set_report_physical(true);
    c.island.set_controlling(true);
    let _cleanup = Cleanup;

    let result = run_loop(
        &key,
        &model,
        &goal,
        &monitor,
        cancel,
        max_steps,
        deadline,
        history_id,
        confirm_timeout,
        unattended,
        settings.agent.debug_screenshots,
    )
    .await;

    let duration = started.elapsed().as_millis() as i64;
    if let Some(id) = history_id {
        let outcome = match &result {
            Ok(s) => s.clone(),
            Err(e) => format!("failed: {e}"),
        };
        let _ = history::finish_command(&c.db, id, &outcome, duration);
        c.emit("history-changed", json!({}));
    }
    tracing::info!(
        ok = result.is_ok(),
        duration_ms = duration,
        "computer use task finished"
    );
    result
}

#[allow(clippy::too_many_arguments)]
async fn run_loop(
    key: &crate::secrets::ApiKey,
    model: &str,
    goal: &str,
    monitor: &MonitorInfo,
    cancel: &CancellationToken,
    max_steps: usize,
    deadline: Instant,
    history_id: Option<i64>,
    confirm_timeout: Duration,
    unattended: bool,
    debug: bool,
) -> Result<String, String> {
    let c = ctx();
    let elevated_msg =
        "The window in front runs as administrator, and Windows blocks Wisp from controlling it";
    if winutil::foreground().elevated {
        return Err(elevated_msg.into());
    }
    let (img, mime, bytes) = screenshot(monitor).await?;
    debug_save(debug, 0, &bytes, mime);
    let mut last_hash = capture::hash(&img);
    let mut body = cu::first_request(model, goal, mime, &bytes);
    let mut history_sig: Vec<(String, [u8; 32])> = vec![];
    let mut step_no = 0usize;

    loop {
        if cancel.is_cancelled() {
            return Err("Stopped".into());
        }
        if Instant::now() > deadline {
            return Err("Stopped: the task hit its time limit".into());
        }
        c.island.set(State::Thinking, Some("Thinking"));
        let resp = c
            .gemini
            .create_interaction(key, &body, TIMEOUT_COMPUTER_USE, cancel)
            .await
            .map_err(|e| {
                if matches!(e, crate::gemini::GemError::Cancelled) {
                    "Stopped".to_string()
                } else {
                    e.code().island_message().to_string()
                }
            })?;
        let turn =
            cu::parse_turn(&resp).map_err(|e| format!("Unexpected response from Gemini: {e}"))?;
        if turn.calls.is_empty() {
            let text = turn.text.trim();
            return Ok(if text.is_empty() {
                "Task finished".to_string()
            } else {
                text.chars().take(500).collect()
            });
        }
        if turn.status == "failed" {
            return Err("Gemini reported the task failed".into());
        }

        let mut results = vec![];
        for call in &turn.calls {
            step_no += 1;
            if step_no > max_steps {
                return Err(format!("Stopped after {max_steps} steps"));
            }
            if !c.agent.wait_if_paused(cancel).await {
                return Err("Stopped".into());
            }
            let fg = winutil::foreground();
            if fg.elevated {
                return Err(elevated_msg.into());
            }
            let intent = call
                .intent
                .clone()
                .unwrap_or_else(|| call.name.replace('_', " "));
            let mut safety_label = call.safety.as_ref().map(|s| s.decision.clone());
            let mut user_answer: Option<&str> = None;
            let mut acknowledged = false;

            let parsed = cu::parse_action(&call.name, &call.args);
            // Safety decision from the model (FR-CU-07). Never auto-approve.
            if let Some(s) = &call.safety {
                match s.decision.as_str() {
                    "blocked" => {
                        log_step(
                            history_id,
                            step_no,
                            &intent,
                            &call.args,
                            Some("blocked"),
                            None,
                        );
                        return Err(format!("Gemini blocked this step: {}", s.explanation));
                    }
                    "require_confirmation" => {
                        if unattended {
                            crate::notify::show("Wisp is waiting for you", &intent);
                        }
                        let yes = c
                            .agent
                            .confirm(
                                ConfirmKind::Safety,
                                &intent,
                                &s.explanation,
                                confirm_timeout,
                                cancel,
                            )
                            .await;
                        user_answer = Some(if yes { "yes" } else { "no" });
                        if !yes {
                            log_step(
                                history_id,
                                step_no,
                                &intent,
                                &call.args,
                                Some("require_confirmation"),
                                user_answer,
                            );
                            return Err(if unattended && !cancel.is_cancelled() {
                                "Aborted: no answer to a confirmation".into()
                            } else {
                                "You declined a step".into()
                            });
                        }
                        acknowledged = true;
                    }
                    _ => {}
                }
            }
            // Independent guard list (FR-CU-08), whatever the model says.
            let guard = c.settings().agent.guard_list;
            if let Some(why) = guard_hit(&fg, &guard) {
                if !acknowledged {
                    if unattended {
                        crate::notify::show("Wisp is waiting for you", &format!("Guard: {why}"));
                    }
                    let yes = c
                        .agent
                        .confirm(
                            ConfirmKind::Guard,
                            &intent,
                            &format!("Paused because {why}. Continue?"),
                            confirm_timeout,
                            cancel,
                        )
                        .await;
                    user_answer = Some(if yes { "yes" } else { "no" });
                    safety_label = Some("guard".into());
                    if !yes {
                        log_step(
                            history_id,
                            step_no,
                            &intent,
                            &call.args,
                            safety_label.as_deref(),
                            user_answer,
                        );
                        return Err("Stopped at a guarded window".into());
                    }
                }
            }

            let outcome: Result<(), String> = match parsed {
                Err(e) => Err(e),
                Ok(action) => {
                    // Stuck detection (FR-CU-06).
                    let sig = serde_json::to_string(&action).unwrap_or_default();
                    history_sig.push((sig, last_hash));
                    if history_sig.len() >= STUCK_REPEATS {
                        let tail = &history_sig[history_sig.len() - STUCK_REPEATS..];
                        if tail.iter().all(|t| *t == tail[0]) {
                            log_step(
                                history_id,
                                step_no,
                                &intent,
                                &call.args,
                                safety_label.as_deref(),
                                user_answer,
                            );
                            return Err(
                                "Stopped: the same step repeated with no change on screen".into()
                            );
                        }
                    }
                    c.island.acting(&intent);
                    let target = action.target().map(|(x, y)| px(monitor, x, y));
                    c.emit("agent-step", json!({ "step": step_no, "intent": intent, "x": target.map(|t| t.0), "y": target.map(|t| t.1) }));
                    if let Some((x, y)) = target {
                        crate::cta::show_ring(monitor, x, y);
                    }
                    let m = monitor.clone();
                    let tok = cancel.clone();
                    let r = tokio::task::spawn_blocking(move || execute(&m, &action, &tok))
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r);
                    let pace = 30 + rand::random::<u64>() % 50;
                    tokio::time::sleep(Duration::from_millis(pace)).await;
                    r
                }
            };
            log_step(
                history_id,
                step_no,
                &intent,
                &call.args,
                safety_label.as_deref(),
                user_answer,
            );
            let mut result = match &outcome {
                Ok(()) => json!({ "status": "ok" }),
                Err(e) => json!({ "status": "error", "error": e }),
            };
            if acknowledged {
                result["safety_acknowledgement"] = json!(true);
            }
            results.push(ActionResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                result,
                is_error: outcome.is_err(),
            });
        }
        if cancel.is_cancelled() {
            return Err("Stopped".into());
        }
        // Let the UI settle before the next screenshot.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let (img, mime, bytes) = screenshot(monitor).await?;
        debug_save(debug, step_no, &bytes, mime);
        last_hash = capture::hash(&img);
        body = cu::next_request(model, &turn.interaction_id, &results, mime, &bytes);
    }
}

fn log_step(
    history_id: Option<i64>,
    step: usize,
    intent: &str,
    args: &Value,
    safety: Option<&str>,
    answer: Option<&str>,
) {
    let Some(id) = history_id else { return };
    let mut a = args.clone();
    if let Some(t) = a.get_mut("text") {
        if let Some(s) = t.as_str() {
            *t = json!(s.chars().take(200).collect::<String>());
        }
    }
    let _ = history::add_step(&ctx().db, id, step as i64, Some(intent), &a, safety, answer);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fg(process: &str, title: &str) -> Foreground {
        Foreground {
            process: process.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    #[test]
    fn guard_matches_process_and_title() {
        let g = GuardList {
            processes: vec!["1Password.exe".into(), "KeePassXC.exe".into()],
            title_keywords: vec!["bank".into(), "checkout".into()],
        };
        assert!(guard_hit(&fg("1password.exe", "Vault"), &g).is_some());
        assert!(guard_hit(&fg("chrome.exe", "HDFC Bank - NetBanking"), &g).is_some());
        assert!(guard_hit(&fg("chrome.exe", "Checkout | Amazon"), &g).is_some());
        assert!(guard_hit(&fg("notepad.exe", "Untitled - Notepad"), &g).is_none());
        assert!(guard_hit(&fg("", ""), &g).is_none());
    }
}
