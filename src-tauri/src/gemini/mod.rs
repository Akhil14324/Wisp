//! Everything that talks to Google lives under this module (section 5).

pub mod client;
pub mod computer_use;
pub mod live;
pub mod prompts;
pub mod schema;

use base64::Engine;
use serde_json::{json, Value};

pub use client::{GemError, GeminiClient};

/// An inline media part for generateContent (`inlineData`).
pub fn inline_part(mime: &str, bytes: &[u8]) -> Value {
    json!({ "inlineData": { "mimeType": mime, "data": base64::engine::general_purpose::STANDARD.encode(bytes) } })
}

pub fn text_part(text: &str) -> Value {
    json!({ "text": text })
}

pub fn system_instruction(text: &str) -> Value {
    json!({ "parts": [{ "text": text }] })
}

/// The first candidate's content, to append unchanged to history so any
/// thought signatures are returned to the model.
pub fn model_content(resp: &Value) -> Option<Value> {
    resp.pointer("/candidates/0/content").cloned()
}

/// Concatenated non-thought text of the first candidate.
pub fn response_text(resp: &Value) -> String {
    resp.pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter(|p| !p.get("thought").and_then(Value::as_bool).unwrap_or(false))
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionCall {
    pub id: Option<String>,
    pub name: String,
    pub args: Value,
}

/// Function calls in the first candidate, in order.
pub fn function_calls(resp: &Value) -> Vec<FunctionCall> {
    resp.pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("functionCall"))
                .map(|fc| FunctionCall {
                    id: fc.get("id").and_then(Value::as_str).map(String::from),
                    name: fc
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    args: fc.get("args").cloned().unwrap_or_else(|| json!({})),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A functionResponse part answering `call`.
pub fn function_response_part(call: &FunctionCall, response: Value) -> Value {
    let mut fr = json!({ "name": call.name, "response": response });
    if let Some(id) = &call.id {
        fr["id"] = json!(id);
    }
    json!({ "functionResponse": fr })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_calls_and_text() {
        let resp = json!({"candidates": [{"content": {"role": "model", "parts": [
            {"text": "thinking", "thought": true},
            {"functionCall": {"id": "c1", "name": "open_app", "args": {"name": "Spotify"}}, "thoughtSignature": "abc"},
            {"text": "Opening."}
        ]}}]});
        let calls = function_calls(&resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args["name"], "Spotify");
        assert_eq!(response_text(&resp), "Opening.");
        let part = function_response_part(&calls[0], json!({"ok": true}));
        assert_eq!(part["functionResponse"]["id"], "c1");
        assert_eq!(
            model_content(&resp).unwrap()["parts"][1]["thoughtSignature"],
            "abc"
        );
    }
}
