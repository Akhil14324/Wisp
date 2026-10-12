//! Gemini REST client: timeouts, retries, 429 backoff, error mapping
//! (FR-GEM-01 to 06). The key travels only in the `x-goog-api-key` header.

use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::error::{ErrCode, UiError};
use crate::secrets::ApiKey;

pub const DEFAULT_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
pub const TIMEOUT: Duration = Duration::from_secs(30);
pub const TIMEOUT_COMPUTER_USE: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RETRIES: u32 = 3;
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);
/// FR-GEM-04: keep any single request under 10 MB.
pub const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, thiserror::Error)]
pub enum GemError {
    #[error("no API key")]
    NoKey,
    #[error("key rejected ({0})")]
    KeyInvalid(u16),
    #[error("rate limited")]
    RateLimit,
    #[error("network error")]
    Network,
    #[error("model not found")]
    ModelNotFound,
    #[error("safety blocked")]
    SafetyBlocked,
    #[error("timeout")]
    Timeout,
    #[error("empty result")]
    EmptyResult,
    #[error("malformed response")]
    Malformed,
    #[error("request too large")]
    TooLarge,
    #[error("http {0}: {1}")]
    Http(u16, String),
    #[error("cancelled")]
    Cancelled,
}

impl GemError {
    pub fn code(&self) -> ErrCode {
        match self {
            GemError::NoKey => ErrCode::NoKey,
            GemError::KeyInvalid(_) => ErrCode::KeyInvalid,
            GemError::RateLimit => ErrCode::RateLimit,
            GemError::Network => ErrCode::Network,
            GemError::ModelNotFound => ErrCode::ModelNotFound,
            GemError::SafetyBlocked => ErrCode::SafetyBlocked,
            GemError::Timeout => ErrCode::Timeout,
            GemError::EmptyResult => ErrCode::EmptyResult,
            GemError::Cancelled => ErrCode::Cancelled,
            GemError::Malformed | GemError::TooLarge | GemError::Http(..) => ErrCode::Internal,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            GemError::KeyInvalid(s) | GemError::Http(s, _) => Some(*s),
            GemError::RateLimit => Some(429),
            GemError::ModelNotFound => Some(404),
            _ => None,
        }
    }
}

impl From<GemError> for UiError {
    fn from(e: GemError) -> Self {
        UiError::of(e.code())
    }
}

/// Counter hooks for FR-SET-09.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientEvent {
    Request,
    RateLimited,
}

pub type Observer = Arc<dyn Fn(ClientEvent) + Send + Sync>;

#[derive(Clone)]
pub struct GeminiClient {
    http: reqwest::Client,
    base: String,
    retry_base: Duration,
    observer: Option<Observer>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub methods: Vec<String>,
}

impl GeminiClient {
    pub fn new(base: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("Wisp/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("http client");
        Self {
            http,
            base: base.into().trim_end_matches('/').to_string(),
            retry_base: Duration::from_secs(1),
            observer: None,
        }
    }

    /// Shorter retry delays for tests.
    pub fn with_retry_base(mut self, d: Duration) -> Self {
        self.retry_base = d;
        self
    }

    pub fn with_observer(mut self, o: Observer) -> Self {
        self.observer = Some(o);
        self
    }

    fn notify(&self, e: ClientEvent) {
        if let Some(o) = &self.observer {
            o(e);
        }
    }

    /// Lists all models available to the key (FR-SET-04, FR-SET-07).
    pub async fn list_models(&self, key: &ApiKey) -> Result<Vec<ModelInfo>, GemError> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        for _ in 0..20 {
            let mut url = format!("{}/models?pageSize=1000", self.base);
            if let Some(p) = &page {
                url.push_str("&pageToken=");
                url.push_str(p);
            }
            let v = self
                .send(
                    key,
                    reqwest::Method::GET,
                    &url,
                    None,
                    TIMEOUT,
                    &CancellationToken::new(),
                )
                .await?;
            for m in v
                .get("models")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                let name = m.get("name").and_then(Value::as_str).unwrap_or_default();
                out.push(ModelInfo {
                    id: name.trim_start_matches("models/").to_string(),
                    display_name: m
                        .get("displayName")
                        .and_then(Value::as_str)
                        .unwrap_or(name)
                        .to_string(),
                    methods: m
                        .get("supportedGenerationMethods")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|s| s.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                });
            }
            page = v
                .get("nextPageToken")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(String::from);
            if page.is_none() {
                break;
            }
        }
        Ok(out)
    }

    /// `models/{model}:generateContent`, with safety-block detection.
    pub async fn generate_content(
        &self,
        key: &ApiKey,
        model: &str,
        body: &Value,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Value, GemError> {
        if !valid_model_id(model) {
            return Err(GemError::ModelNotFound);
        }
        let url = format!("{}/models/{}:generateContent", self.base, model);
        let v = self
            .send(
                key,
                reqwest::Method::POST,
                &url,
                Some(body),
                timeout,
                cancel,
            )
            .await?;
        check_generate_response(&v)?;
        Ok(v)
    }

    /// `interactions` create call used by Computer Use.
    pub async fn create_interaction(
        &self,
        key: &ApiKey,
        body: &Value,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Value, GemError> {
        let url = format!("{}/interactions", self.base);
        self.send(
            key,
            reqwest::Method::POST,
            &url,
            Some(body),
            timeout,
            cancel,
        )
        .await
    }

    async fn send(
        &self,
        key: &ApiKey,
        method: reqwest::Method,
        url: &str,
        body: Option<&Value>,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Value, GemError> {
        let payload = match body {
            Some(b) => {
                let bytes = serde_json::to_vec(b).map_err(|_| GemError::Malformed)?;
                if bytes.len() > MAX_REQUEST_BYTES {
                    return Err(GemError::TooLarge);
                }
                Some(bytes)
            }
            None => None,
        };
        let mut attempt = 0u32;
        loop {
            if cancel.is_cancelled() {
                return Err(GemError::Cancelled);
            }
            self.notify(ClientEvent::Request);
            let mut req = self
                .http
                .request(method.clone(), url)
                .header("x-goog-api-key", key.expose())
                .timeout(timeout);
            if let Some(p) = &payload {
                req = req
                    .header("content-type", "application/json")
                    .body(p.clone());
            }
            let res = tokio::select! {
                _ = cancel.cancelled() => return Err(GemError::Cancelled),
                r = req.send() => r,
            };
            let (retryable, retry_after, err) = match res {
                Err(e) => {
                    let err = if e.is_timeout() {
                        GemError::Timeout
                    } else {
                        GemError::Network
                    };
                    // Timeouts and network errors are not 429/5xx, so they are not retried.
                    return Err(err);
                }
                Ok(resp) => {
                    let status = resp.status();
                    let retry_after = parse_retry_after(resp.headers());
                    let text = tokio::select! {
                        _ = cancel.cancelled() => return Err(GemError::Cancelled),
                        t = resp.text() => t.map_err(|e| if e.is_timeout() { GemError::Timeout } else { GemError::Network })?,
                    };
                    if status.is_success() {
                        return serde_json::from_str(&text).map_err(|_| GemError::Malformed);
                    }
                    let err = map_status(status, &text);
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        self.notify(ClientEvent::RateLimited);
                    }
                    let retryable =
                        status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
                    (retryable, retry_after, err)
                }
            };
            if !retryable || attempt >= MAX_RETRIES {
                return Err(err);
            }
            let delay = retry_after.unwrap_or_else(|| {
                let base = self.retry_base * 2u32.pow(attempt);
                let jitter_ms =
                    rand::random::<u64>() % (self.retry_base.as_millis() as u64 / 4 + 1);
                base + Duration::from_millis(jitter_ms)
            });
            tracing::info!(attempt, delay_ms = delay.as_millis() as u64, "gemini retry");
            tokio::select! {
                _ = cancel.cancelled() => return Err(GemError::Cancelled),
                _ = tokio::time::sleep(delay) => {}
            }
            attempt += 1;
        }
    }
}

pub fn valid_model_id(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 128
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
}

fn parse_retry_after(h: &reqwest::header::HeaderMap) -> Option<Duration> {
    let v = h.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let secs: f64 = v.trim().parse().ok()?;
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    Some(Duration::from_secs_f64(secs).min(MAX_RETRY_AFTER))
}

/// Maps an error status and body to a GemError. Never includes the key.
pub fn map_status(status: StatusCode, body: &str) -> GemError {
    let lower = body.to_ascii_lowercase();
    match status.as_u16() {
        429 => GemError::RateLimit,
        401 | 403 => GemError::KeyInvalid(status.as_u16()),
        400 if lower.contains("api_key_invalid")
            || lower.contains("api key not valid")
            || lower.contains("api key expired") =>
        {
            GemError::KeyInvalid(400)
        }
        404 => GemError::ModelNotFound,
        s => {
            let msg = serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|v| {
                    v.pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(|m| m.chars().take(200).collect())
                })
                .unwrap_or_default();
            GemError::Http(s, msg)
        }
    }
}

const BLOCK_FINISH: &[&str] = &[
    "SAFETY",
    "BLOCKLIST",
    "PROHIBITED_CONTENT",
    "SPII",
    "IMAGE_SAFETY",
    "IMAGE_PROHIBITED_CONTENT",
    "RECITATION",
];

/// Detects prompt blocks and safety stops in a generateContent response.
pub fn check_generate_response(v: &Value) -> Result<(), GemError> {
    if v.pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
        .is_some()
    {
        return Err(GemError::SafetyBlocked);
    }
    let Some(cands) = v.get("candidates").and_then(Value::as_array) else {
        return Err(GemError::Malformed);
    };
    let Some(first) = cands.first() else {
        return Err(GemError::EmptyResult);
    };
    let finish = first
        .get("finishReason")
        .and_then(Value::as_str)
        .unwrap_or("");
    if BLOCK_FINISH.contains(&finish) && first.pointer("/content/parts").is_none() {
        return Err(GemError::SafetyBlocked);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_mapping() {
        assert!(matches!(
            map_status(StatusCode::TOO_MANY_REQUESTS, ""),
            GemError::RateLimit
        ));
        assert!(matches!(
            map_status(
                StatusCode::BAD_REQUEST,
                r#"{"error":{"message":"API key not valid. Please pass a valid API key."}}"#
            ),
            GemError::KeyInvalid(400)
        ));
        assert!(matches!(
            map_status(StatusCode::NOT_FOUND, ""),
            GemError::ModelNotFound
        ));
        assert!(matches!(
            map_status(StatusCode::BAD_REQUEST, r#"{"error":{"message":"bad"}}"#),
            GemError::Http(400, _)
        ));
    }

    #[test]
    fn safety_detection() {
        assert!(matches!(
            check_generate_response(&json!({"promptFeedback": {"blockReason": "SAFETY"}})),
            Err(GemError::SafetyBlocked)
        ));
        assert!(matches!(
            check_generate_response(&json!({"candidates": [{"finishReason": "SAFETY"}]})),
            Err(GemError::SafetyBlocked)
        ));
        assert!(matches!(
            check_generate_response(&json!({"candidates": []})),
            Err(GemError::EmptyResult)
        ));
        assert!(check_generate_response(&json!({"candidates": [{"content": {"parts": [{"text": "hi"}]}, "finishReason": "STOP"}]}))
            .is_ok());
    }

    #[test]
    fn model_ids() {
        assert!(valid_model_id("gemini-3.8-flash"));
        assert!(!valid_model_id("../etc"));
        assert!(!valid_model_id("a/b"));
    }
}
