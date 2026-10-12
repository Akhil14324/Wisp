//! Runs tool calls from the model against the system (section 4.4).
//! Model output is data: every argument is validated before use (NFR-13).

use std::time::Duration;

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::{
    opt_str_arg, parse_due, search_url, str_arg, validate_url, ToolName, MAX_ARG, MAX_TEXT,
};
use crate::agent::tool_loop::ToolHost;
use crate::apps::matching::Resolution;
use crate::input::keys;
use crate::media::{self, MediaAction};
use crate::scheduler::{schedule::When, to_utc_text};
use crate::state::ctx;
use crate::store::{jobs, todos};

pub struct SystemTools {
    allowed: Vec<ToolName>,
    unattended: bool,
    cancel: CancellationToken,
}

impl SystemTools {
    pub fn new(allowed: Vec<ToolName>, unattended: bool, cancel: CancellationToken) -> Self {
        Self {
            allowed,
            unattended,
            cancel,
        }
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

impl ToolHost for SystemTools {
    async fn call(&self, name: &str, args: Value) -> Result<Value, String> {
        let Some(tool) = ToolName::parse(name) else {
            return Err(format!("unknown tool {name}"));
        };
        if !self.allowed.contains(&tool) {
            return Err(format!("{name} is not enabled in the active Loadout"));
        }
        tracing::info!(tool = name, "tool call");
        let r = self.run(tool, &args).await;
        if let Err(e) = &r {
            tracing::info!(tool = name, error = %e, "tool failed");
        }
        r
    }
}

impl SystemTools {
    async fn run(&self, tool: ToolName, args: &Value) -> Result<Value, String> {
        let c = ctx();
        match tool {
            ToolName::OpenApp => {
                let name = str_arg(args, "name", 200)?.to_string();
                match c.apps.resolve(&name) {
                    Resolution::Launch(app) => {
                        let label = app.name.clone();
                        let focused = blocking(move || {
                            let before = crate::winutil::foreground().hwnd;
                            crate::apps::launch(&app)?;
                            // Wait for the new window so a following press_keys
                            // or type_text lands in it, not in the old app.
                            let until = std::time::Instant::now() + Duration::from_secs(4);
                            while std::time::Instant::now() < until {
                                let fg = crate::winutil::foreground();
                                if fg.hwnd != 0 && fg.hwnd != before && !fg.is_wisp {
                                    std::thread::sleep(Duration::from_millis(250));
                                    return Ok(true);
                                }
                                std::thread::sleep(Duration::from_millis(100));
                            }
                            Ok(false)
                        })
                        .await?;
                        Ok(json!({ "launched": label, "window_in_front": focused }))
                    }
                    Resolution::Choose(opts) => {
                        let names: Vec<String> = opts.iter().map(|o| o.name.clone()).collect();
                        if !self.unattended {
                            c.emit("app-choices", json!({ "query": name, "choices": opts }));
                        }
                        Ok(
                            json!({ "launched": null, "ambiguous": names, "note": "Several apps match. The user was shown the options to pick from." }),
                        )
                    }
                    Resolution::NotFound => Err(format!("No installed app matches '{name}'")),
                }
            }
            ToolName::OpenUrl => {
                let url = validate_url(str_arg(args, "url", 2048)?)?;
                let u2 = url.clone();
                blocking(move || crate::apps::open_url(&u2)).await?;
                Ok(json!({ "opened": url }))
            }
            ToolName::WebSearch => {
                let q = str_arg(args, "query", MAX_ARG)?;
                let engine = opt_str_arg(args, "engine", 30)?
                    .map(String::from)
                    .unwrap_or_else(|| c.settings().search.default_engine);
                let url = search_url(&engine, q)?;
                let u2 = url.clone();
                blocking(move || crate::apps::open_url(&u2)).await?;
                Ok(json!({ "opened": url, "engine": engine }))
            }
            ToolName::TypeText => {
                let text = str_arg(args, "text", MAX_TEXT)?.to_string();
                let how = blocking(move || crate::dictation::insert_text(&text)).await?;
                Ok(json!({ "inserted": format!("{how:?}").to_lowercase() }))
            }
            ToolName::PressKeys => {
                let spec = str_arg(args, "keys", 64)?.to_string();
                let chord = keys::parse_chord(&spec).map_err(|e| e.0)?;
                let wisp_focused = crate::winutil::foreground().is_wisp;
                if keys::is_blocked(&chord, wisp_focused) {
                    return Err(format!("{spec} is blocked for safety"));
                }
                blocking(move || {
                    crate::winutil::wait_modifiers_released(Duration::from_millis(1500));
                    crate::input::press_chord(&chord).map_err(anyhow::Error::from)
                })
                .await?;
                Ok(json!({ "pressed": spec }))
            }
            ToolName::ClipboardRead => {
                let t = blocking(crate::clipboard::read_text).await?;
                let t: String = t.unwrap_or_default().chars().take(MAX_TEXT).collect();
                Ok(json!({ "text": t }))
            }
            ToolName::ClipboardWrite => {
                let t = str_arg(args, "text", MAX_TEXT)?.to_string();
                blocking(move || crate::clipboard::write_text(&t)).await?;
                Ok(json!({ "written": true }))
            }
            ToolName::MediaControl => {
                let a = MediaAction::parse(str_arg(args, "action", 20)?)
                    .ok_or("action must be play_pause, next or previous")?;
                blocking(move || media::control(a)).await?;
                Ok(json!({ "done": a }))
            }
            ToolName::SetVolume => {
                let level = match args.get("level") {
                    None | Some(Value::Null) => None,
                    Some(v) => {
                        let n = v.as_f64().ok_or("level must be a number")?;
                        if !(0.0..=100.0).contains(&n) {
                            return Err("level must be 0 to 100".into());
                        }
                        Some(n.round() as u32)
                    }
                };
                let mute = args.get("mute").and_then(Value::as_bool);
                if level.is_none() && mute.is_none() {
                    return Err("give level or mute".into());
                }
                blocking(move || media::set_volume(level, mute)).await?;
                Ok(json!({ "level": level, "mute": mute }))
            }
            ToolName::SetWallpaper => {
                let p = str_arg(args, "path", 500)?.to_string();
                blocking(move || media::set_wallpaper(&p)).await?;
                Ok(json!({ "set": true }))
            }
            ToolName::DescribeScreen => {
                let q = str_arg(args, "question", MAX_ARG)?.to_string();
                let answer = crate::cta::describe_screen(&q).await?;
                Ok(json!({ "answer": answer }))
            }
            ToolName::CreateTodo => {
                let title = str_arg(args, "title", 200)?.trim().to_string();
                let due = match opt_str_arg(args, "due", 40)? {
                    Some(d) => Some(parse_due(d, chrono::Local::now())?),
                    None => None,
                };
                // Goes to TaskHub when signed in, else the local list.
                let t = crate::todolist::create(&todos::NewTodo {
                    title,
                    notes: None,
                    due_at: due,
                })
                .await
                .map_err(|e| e.message)?;
                c.emit("todos-changed", json!({}));
                c.scheduler.wake();
                Ok(json!({ "created": t.title, "due_at": t.due_at, "list": t.source }))
            }
            ToolName::CreateSchedule | ToolName::CreateRoutine => {
                let command = str_arg(args, "command", MAX_ARG)?.trim().to_string();
                let when: When =
                    serde_json::from_value(args.get("when").cloned().unwrap_or(Value::Null))
                        .map_err(|_| "when is missing or has the wrong shape".to_string())?;
                let now = chrono::Local::now();
                when.validate(now)?;
                let name = match tool {
                    ToolName::CreateRoutine => str_arg(args, "name", 80)?.trim().to_string(),
                    _ => opt_str_arg(args, "name", 80)?
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| command.chars().take(60).collect()),
                };
                let next = when.next_after(now).map(to_utc_text);
                // Allow Computer Use is never set by voice (FR-SCH-06, NFR-16).
                let job = jobs::create(&c.db, &name, &command, &when, false, next.clone())
                    .map_err(|e| e.to_string())?;
                c.emit("jobs-changed", json!({}));
                c.scheduler.wake();
                if !self.unattended {
                    // Non-blocking confirmation card (section 4.4).
                    c.emit("job-created", json!({ "id": job.id, "name": job.name, "command": job.command, "when": when.describe() }));
                }
                Ok(json!({ "created": job.name, "schedule": when.describe(), "next_run_at": next }))
            }
            ToolName::RunComputerTask => {
                let goal = str_arg(args, "goal", 1000)?.to_string();
                let summary =
                    crate::agent::computer::run_task(&goal, &self.cancel, self.unattended).await?;
                Ok(json!({ "summary": summary }))
            }
        }
    }
}
