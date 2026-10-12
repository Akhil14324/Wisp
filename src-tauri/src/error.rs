//! The error type every Tauri command returns (section 3.6, FR-GEM-05).
//! Messages are plain language and never contain secrets.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrCode {
    NoKey,
    KeyInvalid,
    RateLimit,
    Network,
    ModelNotFound,
    SafetyBlocked,
    Timeout,
    EmptyResult,
    Internal,
    /// Local validation failure (bad argument, unknown setting, ...).
    Invalid,
    /// A command that only the main window may call (NFR-16).
    Forbidden,
    Cancelled,
}

impl ErrCode {
    /// One-line island message (section 8.4).
    pub fn island_message(self) -> &'static str {
        match self {
            ErrCode::NoKey => "Add your Gemini key in Settings",
            ErrCode::KeyInvalid => "Gemini rejected the key",
            ErrCode::RateLimit => "Gemini rate limit reached, try again shortly",
            ErrCode::Network => "No connection to Gemini",
            ErrCode::ModelNotFound => "Model not available. Check Models in Settings",
            ErrCode::SafetyBlocked => "Gemini declined this request",
            ErrCode::Timeout => "Gemini took too long",
            ErrCode::EmptyResult => "Didn't catch that",
            ErrCode::Internal => "Something went wrong. Details are in the log",
            ErrCode::Invalid => "That request was not valid",
            ErrCode::Forbidden => "That action is only allowed from the main window",
            ErrCode::Cancelled => "Stopped",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ErrCode::NoKey => "NO_KEY",
            ErrCode::KeyInvalid => "KEY_INVALID",
            ErrCode::RateLimit => "RATE_LIMIT",
            ErrCode::Network => "NETWORK",
            ErrCode::ModelNotFound => "MODEL_NOT_FOUND",
            ErrCode::SafetyBlocked => "SAFETY_BLOCKED",
            ErrCode::Timeout => "TIMEOUT",
            ErrCode::EmptyResult => "EMPTY_RESULT",
            ErrCode::Internal => "INTERNAL",
            ErrCode::Invalid => "INVALID",
            ErrCode::Forbidden => "FORBIDDEN",
            ErrCode::Cancelled => "CANCELLED",
        }
    }
}

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct UiError {
    pub code: ErrCode,
    pub message: String,
}

impl UiError {
    pub fn new(code: ErrCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Error carrying the standard island message for its code.
    pub fn of(code: ErrCode) -> Self {
        Self::new(code, code.island_message())
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrCode::Invalid, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrCode::Internal, message)
    }
}

impl From<anyhow::Error> for UiError {
    fn from(e: anyhow::Error) -> Self {
        if let Some(ui) = e.downcast_ref::<UiError>() {
            return ui.clone();
        }
        tracing::error!(error = %e, "internal error");
        UiError::of(ErrCode::Internal)
    }
}

impl From<rusqlite::Error> for UiError {
    fn from(e: rusqlite::Error) -> Self {
        tracing::error!(error = %e, "database error");
        UiError::of(ErrCode::Internal)
    }
}

pub type UiResult<T> = Result<T, UiError>;
