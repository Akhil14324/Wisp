//! Gemini Computer Use (Preview) over the Interactions API, desktop environment.
//! Wire format from https://ai.google.dev/gemini-api/docs/computer-use and
//! https://ai.google.dev/api/interactions-api (read 2026-10-12). Everything that
//! depends on that format is isolated here (assumption 2.3.2).

use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};

use super::prompts;
use crate::input::keys;

pub const MAX_TEXT: usize = 4000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CuAction {
    Click {
        x: u32,
        y: u32,
    },
    DoubleClick {
        x: u32,
        y: u32,
    },
    TripleClick {
        x: u32,
        y: u32,
    },
    MiddleClick {
        x: u32,
        y: u32,
    },
    RightClick {
        x: u32,
        y: u32,
    },
    MouseDown {
        x: u32,
        y: u32,
    },
    MouseUp {
        x: u32,
        y: u32,
    },
    Move {
        x: u32,
        y: u32,
    },
    Type {
        text: String,
        press_enter: bool,
    },
    DragAndDrop {
        start_x: u32,
        start_y: u32,
        end_x: u32,
        end_y: u32,
    },
    Wait {
        seconds: u32,
    },
    PressKey {
        key: String,
    },
    KeyDown {
        key: String,
    },
    KeyUp {
        key: String,
    },
    Hotkey {
        keys: Vec<String>,
    },
    TakeScreenshot,
    Scroll {
        x: u32,
        y: u32,
        direction: String,
        magnitude: u32,
    },
}

impl CuAction {
    /// The normalized point this action targets, for the feedback ring.
    pub fn target(&self) -> Option<(u32, u32)> {
        use CuAction::*;
        match self {
            Click { x, y }
            | DoubleClick { x, y }
            | TripleClick { x, y }
            | MiddleClick { x, y }
            | RightClick { x, y }
            | MouseDown { x, y }
            | MouseUp { x, y }
            | Move { x, y }
            | Scroll { x, y, .. } => Some((*x, *y)),
            DragAndDrop {
                start_x, start_y, ..
            } => Some((*start_x, *start_y)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Safety {
    pub decision: String,
    pub explanation: String,
}

#[derive(Debug, Clone)]
pub struct CuCall {
    pub id: String,
    pub name: String,
    pub args: Value,
    pub intent: Option<String>,
    pub safety: Option<Safety>,
}

#[derive(Debug, Clone)]
pub struct CuTurn {
    pub interaction_id: String,
    pub status: String,
    pub calls: Vec<CuCall>,
    pub text: String,
}

/// Prompt-injection detection is documented for 3.5 Flash-Lite or later (FR-CU-09).
pub fn supports_injection_detection(model: &str) -> bool {
    let rest = match model.strip_prefix("gemini-") {
        Some(r) => r,
        None => return false,
    };
    let ver: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut it = ver.split('.');
    let major: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor) >= (3, 5)
}

fn tool(model: &str) -> Value {
    json!([{
        "type": "computer_use",
        "environment": "desktop",
        "enable_prompt_injection_detection": supports_injection_detection(model)
    }])
}

fn image_content(mime: &str, bytes: &[u8]) -> Value {
    json!({ "type": "image", "mime_type": mime, "data": base64::engine::general_purpose::STANDARD.encode(bytes) })
}

/// First request: the goal and the current screenshot.
pub fn first_request(model: &str, goal: &str, mime: &str, screenshot: &[u8]) -> Value {
    json!({
        "model": model,
        "system_instruction": prompts::COMPUTER_USE,
        "tools": tool(model),
        "input": [
            { "type": "text", "text": goal },
            image_content(mime, screenshot)
        ]
    })
}

pub struct ActionResult {
    pub call_id: String,
    pub name: String,
    /// Small JSON object describing the outcome; serialized into the text part.
    pub result: Value,
    pub is_error: bool,
}

/// Follow-up request: one function_result per call, the new screenshot attached
/// to each result (the documented sample attaches it per call).
pub fn next_request(
    model: &str,
    previous_id: &str,
    results: &[ActionResult],
    mime: &str,
    screenshot: &[u8],
) -> Value {
    let input: Vec<Value> = results
        .iter()
        .map(|r| {
            let mut step = json!({
                "type": "function_result",
                "call_id": r.call_id,
                "name": r.name,
                "result": [
                    { "type": "text", "text": r.result.to_string() },
                    image_content(mime, screenshot)
                ]
            });
            if r.is_error {
                step["is_error"] = json!(true);
            }
            step
        })
        .collect();
    json!({
        "model": model,
        "previous_interaction_id": previous_id,
        "system_instruction": prompts::COMPUTER_USE,
        "tools": tool(model),
        "input": input
    })
}

/// Reads the interaction resource. Unknown step types are ignored.
pub fn parse_turn(v: &Value) -> Result<CuTurn, String> {
    let id = v
        .get("id")
        .and_then(Value::as_str)
        .ok_or("response has no id")?
        .to_string();
    let status = v
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut calls = vec![];
    let mut text = String::new();
    for step in v
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        match step.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                let args = step.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let safety = args.get("safety_decision").map(|s| Safety {
                    decision: s
                        .get("decision")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    explanation: s
                        .get("explanation")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .chars()
                        .take(500)
                        .collect(),
                });
                calls.push(CuCall {
                    id: step
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    name: step
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    intent: args
                        .get("intent")
                        .and_then(Value::as_str)
                        .map(|s| s.chars().take(300).collect()),
                    safety,
                    args,
                });
            }
            Some("model_output") => {
                for c in step
                    .get("content")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                {
                    if c.get("type").and_then(Value::as_str) == Some("text") {
                        text.push_str(c.get("text").and_then(Value::as_str).unwrap_or(""));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(CuTurn {
        interaction_id: id,
        status,
        calls,
        text,
    })
}

fn coord(args: &Value, k: &str) -> Result<u32, String> {
    let n = args
        .get(k)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("missing {k}"))?;
    if !(0.0..=999.0).contains(&n) {
        return Err(format!("{k} out of range 0-999"));
    }
    Ok(n.round() as u32)
}

fn key_arg(args: &Value, k: &str) -> Result<String, String> {
    let s = args
        .get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing {k}"))?;
    keys::parse_chord(s).map_err(|e| e.to_string())?;
    Ok(s.to_string())
}

/// Validates a proposed action against the documented desktop action set
/// (section 5.7.4: coordinates 0-999, text under 4,000 chars, key allowlist).
pub fn parse_action(name: &str, args: &Value) -> Result<CuAction, String> {
    let xy = || -> Result<(u32, u32), String> { Ok((coord(args, "x")?, coord(args, "y")?)) };
    Ok(match name {
        "click" => {
            let (x, y) = xy()?;
            CuAction::Click { x, y }
        }
        "double_click" => {
            let (x, y) = xy()?;
            CuAction::DoubleClick { x, y }
        }
        "triple_click" => {
            let (x, y) = xy()?;
            CuAction::TripleClick { x, y }
        }
        "middle_click" => {
            let (x, y) = xy()?;
            CuAction::MiddleClick { x, y }
        }
        "right_click" => {
            let (x, y) = xy()?;
            CuAction::RightClick { x, y }
        }
        "mouse_down" => {
            let (x, y) = xy()?;
            CuAction::MouseDown { x, y }
        }
        "mouse_up" => {
            let (x, y) = xy()?;
            CuAction::MouseUp { x, y }
        }
        "move" => {
            let (x, y) = xy()?;
            CuAction::Move { x, y }
        }
        "type" => {
            let text = args
                .get("text")
                .and_then(Value::as_str)
                .ok_or("missing text")?;
            if text.chars().count() > MAX_TEXT {
                return Err("text longer than 4000 characters".into());
            }
            CuAction::Type {
                text: text.to_string(),
                press_enter: args
                    .get("press_enter")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            }
        }
        "drag_and_drop" => CuAction::DragAndDrop {
            start_x: coord(args, "start_x")?,
            start_y: coord(args, "start_y")?,
            end_x: coord(args, "end_x")?,
            end_y: coord(args, "end_y")?,
        },
        "wait" => CuAction::Wait {
            seconds: args
                .get("seconds")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(0, 30) as u32,
        },
        "press_key" => CuAction::PressKey {
            key: key_arg(args, "key")?,
        },
        "key_down" => CuAction::KeyDown {
            key: key_arg(args, "key")?,
        },
        "key_up" => CuAction::KeyUp {
            key: key_arg(args, "key")?,
        },
        "hotkey" => {
            let ks: Vec<String> = args
                .get("keys")
                .and_then(Value::as_array)
                .ok_or("missing keys")?
                .iter()
                .filter_map(|k| k.as_str().map(String::from))
                .collect();
            if ks.is_empty() || ks.len() > 5 {
                return Err("hotkey needs 1 to 5 keys".into());
            }
            keys::parse_chord(&ks.join("+")).map_err(|e| e.to_string())?;
            CuAction::Hotkey { keys: ks }
        }
        "take_screenshot" => CuAction::TakeScreenshot,
        "scroll" => {
            let (x, y) = xy()?;
            let direction = args
                .get("direction")
                .and_then(Value::as_str)
                .unwrap_or("down")
                .to_string();
            if !["up", "down", "left", "right"].contains(&direction.as_str()) {
                return Err("bad scroll direction".into());
            }
            let magnitude = args
                .get("magnitude_in_pixels")
                .and_then(Value::as_u64)
                .unwrap_or(300)
                .min(999) as u32;
            CuAction::Scroll {
                x,
                y,
                direction,
                magnitude,
            }
        }
        other => return Err(format!("unsupported action {other}")),
    })
}

/// FR-CU-04: normalized 0-999 coordinate to a physical pixel on the monitor.
pub fn to_pixel(n: u32, size: u32, origin: i32) -> i32 {
    origin + ((n as f64 / 1000.0) * size as f64).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injection_detection_by_version() {
        assert!(supports_injection_detection("gemini-3.8-flash"));
        assert!(supports_injection_detection("gemini-3.5-flash-lite"));
        assert!(!supports_injection_detection("gemini-3-flash-preview"));
        assert!(!supports_injection_detection("gemini-2.5-flash"));
    }

    #[test]
    fn coordinate_conversion() {
        assert_eq!(to_pixel(0, 1920, 0), 0);
        assert_eq!(to_pixel(500, 1920, 0), 960);
        assert_eq!(to_pixel(999, 1920, 0), 1918);
        assert_eq!(to_pixel(500, 2560, -2560), -1280);
        assert_eq!(to_pixel(250, 1080, 100), 370);
    }

    #[test]
    fn action_validation() {
        assert_eq!(
            parse_action("click", &json!({"x": 10, "y": 20})).unwrap(),
            CuAction::Click { x: 10, y: 20 }
        );
        assert!(parse_action("click", &json!({"x": 1000, "y": 20})).is_err());
        assert!(parse_action("click", &json!({"x": -1, "y": 20})).is_err());
        assert!(parse_action("type", &json!({"text": "x".repeat(4001)})).is_err());
        assert!(parse_action("hotkey", &json!({"keys": ["Control", "a"]})).is_ok());
        assert!(parse_action("press_key", &json!({"key": "Launch_Missiles"})).is_err());
        assert!(parse_action("navigate", &json!({"url": "x"})).is_err());
        assert!(parse_action("scroll", &json!({"x": 1, "y": 1, "direction": "sideways"})).is_err());
    }

    #[test]
    fn parses_interaction() {
        let v = json!({
            "id": "int_1", "status": "requires_action",
            "steps": [
                {"type": "thought"},
                {"type": "function_call", "id": "call_1", "name": "click",
                 "arguments": {"x": 5, "y": 6, "intent": "Open Start", "safety_decision": {"decision": "require_confirmation", "explanation": "Payment page"}}}
            ]
        });
        let t = parse_turn(&v).unwrap();
        assert_eq!(t.interaction_id, "int_1");
        assert_eq!(t.calls[0].intent.as_deref(), Some("Open Start"));
        assert_eq!(
            t.calls[0].safety.as_ref().unwrap().decision,
            "require_confirmation"
        );
    }

    #[test]
    fn request_shapes() {
        let first = first_request("gemini-3.8-flash", "Open Notepad", "image/png", b"png");
        assert_eq!(first["tools"][0]["environment"], "desktop");
        assert_eq!(first["tools"][0]["enable_prompt_injection_detection"], true);
        assert_eq!(first["input"][1]["type"], "image");
        let next = next_request(
            "gemini-3.8-flash",
            "int_1",
            &[ActionResult {
                call_id: "c".into(),
                name: "click".into(),
                result: json!({"ok": true}),
                is_error: false,
            }],
            "image/png",
            b"png",
        );
        assert_eq!(next["previous_interaction_id"], "int_1");
        assert_eq!(next["input"][0]["type"], "function_result");
        assert_eq!(next["input"][0]["call_id"], "c");
    }
}
