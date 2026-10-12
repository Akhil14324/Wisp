//! Credential Manager access (FR-SET-05, NFR-10). The only place the key is read.

use std::fmt;

const TARGET: &str = "Wisp/GeminiApiKey";
const SERVICE: &str = "Wisp";
const USER: &str = "GeminiApiKey";

/// The Gemini API key. `Debug` and `Display` never reveal it.
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(s: impl Into<String>) -> Self {
        ApiKey(s.into())
    }

    /// Only the Gemini client calls this, to set the request header.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The last 4 characters, for the masked display.
    pub fn masked(&self) -> String {
        let tail: String = self
            .0
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("••••{tail}")
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(***)")
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

fn entry() -> keyring::Result<keyring::Entry> {
    keyring::Entry::new_with_target(TARGET, SERVICE, USER)
}

pub fn get() -> Option<ApiKey> {
    match entry().and_then(|e| e.get_password()) {
        Ok(k) if !k.trim().is_empty() => Some(ApiKey(k)),
        Ok(_) | Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            tracing::warn!(error = %redact(&e), "credential read failed");
            None
        }
    }
}

pub fn set(key: &ApiKey) -> anyhow::Result<()> {
    entry()
        .and_then(|e| e.set_password(key.expose()))
        .map_err(|e| anyhow::anyhow!("credential write failed: {}", redact(&e)))
}

pub fn delete() -> anyhow::Result<()> {
    match entry().and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("credential delete failed: {}", redact(&e))),
    }
}

/// Other Wisp credentials (TaskHub sign-in), each its own Credential Manager
/// entry `Wisp/<name>`. Values never reach logs or the front end.
pub mod named {
    use super::{redact, ApiKey};

    fn entry(name: &str) -> keyring::Result<keyring::Entry> {
        keyring::Entry::new_with_target(&format!("Wisp/{name}"), "Wisp", name)
    }

    pub fn get(name: &str) -> Option<ApiKey> {
        match entry(name).and_then(|e| e.get_password()) {
            Ok(v) if !v.is_empty() => Some(ApiKey::new(v)),
            Ok(_) | Err(keyring::Error::NoEntry) => None,
            Err(e) => {
                tracing::warn!(error = %redact(&e), name, "credential read failed");
                None
            }
        }
    }

    pub fn set(name: &str, value: &str) -> anyhow::Result<()> {
        entry(name)
            .and_then(|e| e.set_password(value))
            .map_err(|e| anyhow::anyhow!("credential write failed: {}", redact(&e)))
    }

    pub fn delete(name: &str) {
        let _ = entry(name).and_then(|e| e.delete_credential());
    }
}

/// keyring errors never contain the secret, but keep only the variant name to be safe.
fn redact(e: &keyring::Error) -> &'static str {
    match e {
        keyring::Error::PlatformFailure(_) => "platform failure",
        keyring::Error::NoStorageAccess(_) => "no storage access",
        keyring::Error::NoEntry => "no entry",
        keyring::Error::BadEncoding(_) => "bad encoding",
        keyring::Error::TooLong(..) => "too long",
        keyring::Error::Invalid(..) => "invalid",
        keyring::Error::Ambiguous(_) => "ambiguous",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_never_prints() {
        let k = ApiKey::new("AIzaSySECRETVALUE1234");
        assert_eq!(format!("{k:?}"), "ApiKey(***)");
        assert_eq!(format!("{k}"), "***");
        assert_eq!(k.masked(), "••••1234");
    }
}
