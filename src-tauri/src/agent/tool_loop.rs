//! The command-mode agent loop (FR-CMD-02, FR-CMD-08): call the model, run
//! each function call with a local tool, send results back, repeat until the
//! model answers with text or 8 rounds have run. Independent of Tauri so it
//! can be tested against a mock Gemini server.

use std::future::Future;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::gemini::{self, GemError, GeminiClient};
use crate::secrets::ApiKey;

pub const MAX_ROUNDS: usize = 8;
pub const MAX_CONSECUTIVE_FAILURES: usize = 2;

/// Runs tools by name. Errors are returned to the model as function responses.
pub trait ToolHost: Send + Sync {
    fn call(&self, name: &str, args: Value) -> impl Future<Output = Result<Value, String>> + Send;
}

#[derive(Debug, Clone, Serialize)]
pub struct CallRecord {
    pub name: String,
    pub args: Value,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug)]
pub enum LoopEnd {
    /// The model replied with text.
    Text(String),
    /// Two tools failed in a row (FR-CMD-08).
    ToolFailures(String),
    /// 8 rounds ran without a final text.
    RoundLimit,
    Gemini(GemError),
}

#[derive(Debug)]
pub struct LoopOutcome {
    pub end: LoopEnd,
    pub calls: Vec<CallRecord>,
}

pub struct LoopRequest<'a> {
    pub model: &'a str,
    pub system: &'a str,
    /// Parts of the first user turn (audio or text).
    pub user_parts: Vec<Value>,
    pub tools: Option<Value>,
    pub timeout: Duration,
}

/// Shortens argument values for history (no audio or long text is stored).
fn summarize_args(v: &Value) -> Value {
    match v {
        Value::String(s) if s.chars().count() > 200 => {
            Value::String(format!("{}…", s.chars().take(200).collect::<String>()))
        }
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| (k.clone(), summarize_args(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub async fn run<H: ToolHost>(
    client: &GeminiClient,
    key: &ApiKey,
    req: LoopRequest<'_>,
    host: &H,
    cancel: &CancellationToken,
) -> LoopOutcome {
    let mut contents: Vec<Value> = vec![json!({ "role": "user", "parts": req.user_parts })];
    let mut calls_log = vec![];
    let mut consecutive_failures = 0usize;
    for _round in 0..MAX_ROUNDS {
        let mut body = json!({
            "systemInstruction": gemini::system_instruction(req.system),
            "contents": contents,
        });
        if let Some(t) = &req.tools {
            body["tools"] = t.clone();
        }
        let resp = match client
            .generate_content(key, req.model, &body, req.timeout, cancel)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return LoopOutcome {
                    end: LoopEnd::Gemini(e),
                    calls: calls_log,
                }
            }
        };
        let calls = gemini::function_calls(&resp);
        if calls.is_empty() {
            let text = gemini::response_text(&resp);
            if text.trim().is_empty() {
                return LoopOutcome {
                    end: LoopEnd::Gemini(GemError::EmptyResult),
                    calls: calls_log,
                };
            }
            return LoopOutcome {
                end: LoopEnd::Text(text.trim().to_string()),
                calls: calls_log,
            };
        }
        let Some(model_turn) = gemini::model_content(&resp) else {
            return LoopOutcome {
                end: LoopEnd::Gemini(GemError::Malformed),
                calls: calls_log,
            };
        };
        contents.push(model_turn);
        let mut response_parts = vec![];
        for call in &calls {
            if cancel.is_cancelled() {
                return LoopOutcome {
                    end: LoopEnd::Gemini(GemError::Cancelled),
                    calls: calls_log,
                };
            }
            let result = host.call(&call.name, call.args.clone()).await;
            let (response, ok, err) = match result {
                Ok(v) => {
                    consecutive_failures = 0;
                    (json!({ "result": v }), true, None)
                }
                Err(e) => {
                    consecutive_failures += 1;
                    (json!({ "error": e }), false, Some(e))
                }
            };
            calls_log.push(CallRecord {
                name: call.name.clone(),
                args: summarize_args(&call.args),
                ok,
                error: err.clone(),
            });
            response_parts.push(gemini::function_response_part(call, response));
            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                return LoopOutcome {
                    end: LoopEnd::ToolFailures(err.unwrap_or_default()),
                    calls: calls_log,
                };
            }
        }
        contents.push(json!({ "role": "user", "parts": response_parts }));
    }
    LoopOutcome {
        end: LoopEnd::RoundLimit,
        calls: calls_log,
    }
}
