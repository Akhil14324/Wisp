//! Settings stored as JSON values by dotted key (section 6.4, Appendix A).

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::Db;
use crate::error::{UiError, UiResult};

/// Appendix A defaults. Every settable key is a leaf of this tree.
pub fn defaults() -> Value {
    json!({
      "schema_version": 1,
      "models": {
        "dictation": "gemini-3.5-flash-lite",
        "command": "gemini-3.8-flash",
        "vision": "gemini-3.8-flash",
        "computer_use": "gemini-3.8-flash",
        "live": null
      },
      "hotkeys": {
        "dictation": "ctrl+win",
        "circle": "ctrl+shift",
        "command": "ctrl+alt+space",
        "emergency_stop": "esc*3"
      },
      "search": { "default_engine": "google" },
      "audio": { "input_device": null },
      "capture": { "target_monitor": "primary" },
      "island": { "monitor": "primary", "hide_in_fullscreen": true },
      "dictation": { "dictionary": [], "history_enabled": true },
      "clipboard": { "history_enabled": false },
      "scheduler": { "grace_minutes": 15 },
      "agent": {
        "guard_list": {
          "processes": ["1Password.exe", "Bitwarden.exe", "KeePass.exe", "KeePassXC.exe"],
          "title_keywords": ["bank", "payment", "checkout"]
        },
        "debug_screenshots": false,
        "max_steps": 40,
        "max_minutes": 10
      },
      "app": { "autostart": true, "first_run_done": false, "privacy_ack_at": null },
      "wallpaper": { "folder": null }
    })
}

/// Keys that can be written (section 6.4). Objects like `agent.guard_list` are leaves.
pub const KEYS: &[&str] = &[
    "models.dictation",
    "models.command",
    "models.vision",
    "models.computer_use",
    "models.live",
    "hotkeys.dictation",
    "hotkeys.circle",
    "hotkeys.command",
    "hotkeys.emergency_stop",
    "search.default_engine",
    "audio.input_device",
    "capture.target_monitor",
    "island.monitor",
    "island.hide_in_fullscreen",
    "dictation.dictionary",
    "dictation.history_enabled",
    "clipboard.history_enabled",
    "scheduler.grace_minutes",
    "agent.guard_list",
    "agent.debug_screenshots",
    "agent.max_steps",
    "agent.max_minutes",
    "app.autostart",
    "app.first_run_done",
    "app.privacy_ack_at",
    "wallpaper.folder",
];

/// Keys only the main window may change (NFR-16, Appendix B).
pub const MAIN_ONLY_KEYS: &[&str] = &["agent.guard_list"];

/// Allowed hotkey chords (FR-HK-06).
pub const ALLOWED_CHORDS: &[&str] = &[
    "ctrl+win",
    "ctrl+shift",
    "ctrl+alt+space",
    "ctrl+alt",
    "alt+shift",
    "win+alt",
    "win+shift",
    "ctrl+win+alt",
    "ctrl+shift+space",
];

pub const SEARCH_ENGINES: &[&str] = &[
    "google",
    "youtube",
    "amazon",
    "wikipedia",
    "bing",
    "duckduckgo",
    "github",
    "maps",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub models: Models,
    pub hotkeys: Hotkeys,
    pub search: Search,
    pub audio: Audio,
    pub capture: Capture,
    pub island: IslandCfg,
    pub dictation: Dictation,
    pub clipboard: ClipboardCfg,
    pub scheduler: SchedulerCfg,
    pub agent: AgentCfg,
    pub app: AppCfg,
    pub wallpaper: WallpaperCfg,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Models {
    pub dictation: String,
    pub command: String,
    pub vision: String,
    pub computer_use: String,
    pub live: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hotkeys {
    pub dictation: String,
    pub circle: String,
    pub command: String,
    pub emergency_stop: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Search {
    pub default_engine: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Audio {
    pub input_device: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capture {
    /// "primary" or a monitor id from `monitors_list`.
    pub target_monitor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandCfg {
    pub monitor: String,
    pub hide_in_fullscreen: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dictation {
    pub dictionary: Vec<String>,
    pub history_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardCfg {
    pub history_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerCfg {
    pub grace_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardList {
    pub processes: Vec<String>,
    pub title_keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCfg {
    pub guard_list: GuardList,
    pub debug_screenshots: bool,
    pub max_steps: u32,
    pub max_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppCfg {
    pub autostart: bool,
    pub first_run_done: bool,
    pub privacy_ack_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperCfg {
    pub folder: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        serde_json::from_value(defaults()).expect("defaults deserialize")
    }
}

/// Sets `value` at a dotted path inside `tree`, creating objects as needed.
fn set_path(tree: &mut Value, key: &str, value: Value) {
    let mut cur = tree;
    let parts: Vec<&str> = key.split('.').collect();
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let Value::Object(map) = cur {
                map.insert((*part).to_string(), value);
            }
            return;
        }
        if !cur.get(*part).map(Value::is_object).unwrap_or(false) {
            if let Value::Object(map) = cur {
                map.insert((*part).to_string(), Value::Object(Map::new()));
            }
        }
        cur = cur.get_mut(*part).expect("just inserted");
    }
}

pub fn get_path<'a>(tree: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(tree, |cur, part| cur.get(part))
}

/// Validates one key/value pair beyond type checks.
pub fn validate(key: &str, value: &Value) -> UiResult<()> {
    if !KEYS.contains(&key) {
        return Err(UiError::invalid(format!("Unknown setting {key}")));
    }
    let bad = |m: &str| Err(UiError::invalid(m.to_string()));
    match key {
        k if k.starts_with("hotkeys.") && k != "hotkeys.emergency_stop" => {
            let s = value.as_str().unwrap_or_default();
            if !ALLOWED_CHORDS.contains(&s) {
                return bad("That chord is not in the allowed list");
            }
        }
        "hotkeys.emergency_stop" => {
            if value.as_str() != Some("esc*3") {
                return bad("The emergency stop is fixed to Esc pressed 3 times");
            }
        }
        "search.default_engine" => {
            if !SEARCH_ENGINES.contains(&value.as_str().unwrap_or_default()) {
                return bad("Unknown search engine");
            }
        }
        "scheduler.grace_minutes" => match value.as_u64() {
            Some(n) if n <= 240 => {}
            _ => return bad("Grace window must be 0 to 240 minutes"),
        },
        "agent.max_steps" => match value.as_u64() {
            Some(n) if (1..=40).contains(&n) => {}
            _ => return bad("Max steps must be 1 to 40"),
        },
        "agent.max_minutes" => match value.as_u64() {
            Some(n) if (1..=10).contains(&n) => {}
            _ => return bad("Max minutes must be 1 to 10"),
        },
        "dictation.dictionary" => {
            let arr = value
                .as_array()
                .ok_or_else(|| UiError::invalid("Dictionary must be a list"))?;
            if arr.len() > 200 {
                return bad("The dictionary holds at most 200 terms");
            }
            if arr.iter().any(|t| {
                t.as_str()
                    .map(|s| s.is_empty() || s.len() > 64)
                    .unwrap_or(true)
            }) {
                return bad("Each dictionary term must be 1 to 64 characters");
            }
        }
        k if k.starts_with("models.") && k != "models.live" => {
            let s = value.as_str().unwrap_or_default();
            if s.is_empty()
                || s.len() > 128
                || !s
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
            {
                return bad("Model IDs use letters, digits, dots, dashes and underscores");
            }
        }
        _ => {}
    }
    Ok(())
}

/// Loads settings: defaults overlaid with stored keys.
pub fn load(db: &Db) -> Settings {
    let tree = load_tree(db);
    serde_json::from_value(tree).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "stored settings invalid, using defaults");
        Settings::default()
    })
}

pub fn load_tree(db: &Db) -> Value {
    let mut tree = defaults();
    let rows: Vec<(String, String)> = db
        .with(|c| {
            let mut st = c.prepare("SELECT key, value_json FROM settings")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
        .unwrap_or_default();
    for (k, v) in rows {
        if !KEYS.contains(&k.as_str()) {
            continue;
        }
        if let Ok(val) = serde_json::from_str(&v) {
            set_path(&mut tree, &k, val);
        }
    }
    tree
}

/// Validates and stores one setting, returning the new typed settings.
pub fn set(db: &Db, key: &str, value: Value) -> UiResult<Settings> {
    validate(key, &value)?;
    let mut tree = load_tree(db);
    set_path(&mut tree, key, value.clone());
    let typed: Settings = serde_json::from_value(tree)
        .map_err(|_| UiError::invalid(format!("Wrong value type for {key}")))?;
    let json = serde_json::to_string(&value).map_err(|e| UiError::internal(e.to_string()))?;
    db.with(|c| {
        c.execute(
            "INSERT INTO settings (key, value_json) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            rusqlite::params![key, json],
        )
    })?;
    Ok(typed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_every_key() {
        let d = defaults();
        for k in KEYS {
            assert!(get_path(&d, k).is_some(), "missing default for {k}");
        }
        let _ = Settings::default();
    }

    #[test]
    fn set_and_reload() {
        let db = Db::open_in_memory().unwrap();
        let s = set(&db, "scheduler.grace_minutes", json!(30)).unwrap();
        assert_eq!(s.scheduler.grace_minutes, 30);
        assert_eq!(load(&db).scheduler.grace_minutes, 30);
    }

    #[test]
    fn rejects_bad_values() {
        let db = Db::open_in_memory().unwrap();
        assert!(set(&db, "nope.key", json!(1)).is_err());
        assert!(set(&db, "hotkeys.dictation", json!("ctrl+q")).is_err());
        assert!(set(&db, "scheduler.grace_minutes", json!("ten")).is_err());
        assert!(set(&db, "agent.max_steps", json!(500)).is_err());
        assert!(set(&db, "app.autostart", json!("yes")).is_err());
        assert!(set(&db, "models.command", json!("bad id!")).is_err());
    }
}
