//! Local tools the model can call (section 4.4). This file holds the tool
//! names and pure validation; `exec` runs them against the system.

pub mod exec;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolName {
    OpenApp,
    OpenUrl,
    WebSearch,
    TypeText,
    PressKeys,
    ClipboardRead,
    ClipboardWrite,
    MediaControl,
    SetVolume,
    SetWallpaper,
    DescribeScreen,
    CreateSchedule,
    CreateRoutine,
    RunComputerTask,
    CreateTodo,
}

impl ToolName {
    /// Declaration order follows the milestones (section 4.4).
    pub const ALL: &'static [ToolName] = &[
        ToolName::OpenApp,
        ToolName::OpenUrl,
        ToolName::WebSearch,
        ToolName::TypeText,
        ToolName::PressKeys,
        ToolName::ClipboardRead,
        ToolName::ClipboardWrite,
        ToolName::MediaControl,
        ToolName::SetVolume,
        ToolName::SetWallpaper,
        ToolName::DescribeScreen,
        ToolName::CreateSchedule,
        ToolName::CreateRoutine,
        ToolName::RunComputerTask,
        ToolName::CreateTodo,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ToolName::OpenApp => "open_app",
            ToolName::OpenUrl => "open_url",
            ToolName::WebSearch => "web_search",
            ToolName::TypeText => "type_text",
            ToolName::PressKeys => "press_keys",
            ToolName::ClipboardRead => "clipboard_read",
            ToolName::ClipboardWrite => "clipboard_write",
            ToolName::MediaControl => "media_control",
            ToolName::SetVolume => "set_volume",
            ToolName::SetWallpaper => "set_wallpaper",
            ToolName::DescribeScreen => "describe_screen",
            ToolName::CreateSchedule => "create_schedule",
            ToolName::CreateRoutine => "create_routine",
            ToolName::RunComputerTask => "run_computer_task",
            ToolName::CreateTodo => "create_todo",
        }
    }

    pub fn parse(s: &str) -> Option<ToolName> {
        ToolName::ALL.iter().copied().find(|t| t.as_str() == s)
    }
}

pub const MAX_TEXT: usize = 4000;
pub const MAX_ARG: usize = 500;

/// Only http and https URLs may be opened (FR-CMD-06, NFR-13).
pub fn validate_url(url: &str) -> Result<String, String> {
    let u = url.trim();
    if u.len() > 2048 {
        return Err("URL is too long".into());
    }
    let lower = u.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .ok_or_else(|| "only http and https URLs can be opened".to_string())?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty()
        || host.contains('@')
        || u.chars().any(|c| c.is_control() || c == ' ' || c == '"')
    {
        return Err("that URL is not valid".into());
    }
    Ok(u.to_string())
}

fn encode(q: &str) -> String {
    let mut out = String::new();
    for b in q.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Builds a search URL for a named engine (FR-CMD-06).
pub fn search_url(engine: &str, query: &str) -> Result<String, String> {
    let q = encode(query);
    if q.is_empty() {
        return Err("empty search query".into());
    }
    let url = match engine.trim().to_ascii_lowercase().as_str() {
        "google" | "" => format!("https://www.google.com/search?q={q}"),
        "youtube" => format!("https://www.youtube.com/results?search_query={q}"),
        "amazon" => format!("https://www.amazon.in/s?k={q}"),
        "wikipedia" => format!("https://en.wikipedia.org/w/index.php?search={q}"),
        "bing" => format!("https://www.bing.com/search?q={q}"),
        "duckduckgo" | "ddg" => format!("https://duckduckgo.com/?q={q}"),
        "github" => format!("https://github.com/search?q={q}"),
        "maps" | "google maps" => format!("https://www.google.com/maps/search/{q}"),
        other => return Err(format!("unknown search engine '{other}'")),
    };
    Ok(url)
}

/// Reads a required string argument with a size limit (NFR-13).
pub fn str_arg<'a>(args: &'a Value, key: &str, max: usize) -> Result<&'a str, String> {
    let s = args
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing '{key}'"))?;
    if s.trim().is_empty() {
        return Err(format!("'{key}' is empty"));
    }
    if s.chars().count() > max {
        return Err(format!("'{key}' is longer than {max} characters"));
    }
    Ok(s)
}

pub fn opt_str_arg<'a>(args: &'a Value, key: &str, max: usize) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) if s.chars().count() <= max => Ok(Some(s.as_str())),
        Some(Value::String(_)) => Err(format!("'{key}' is too long")),
        Some(_) => Err(format!("'{key}' must be a string")),
    }
}

/// Parses a to-do due time "YYYY-MM-DDTHH:MM" (local) into UTC ISO text.
/// A bare "HH:MM" means today if still ahead, else tomorrow (FR-TODO-02 AC).
pub fn parse_due(s: &str, now: chrono::DateTime<chrono::Local>) -> Result<String, String> {
    use chrono::{Local, NaiveDateTime, NaiveTime, TimeZone};
    let s = s.trim();
    let local = if let Ok(t) = NaiveTime::parse_from_str(s, "%H:%M") {
        let today = now.date_naive().and_time(t);
        let at = Local
            .from_local_datetime(&today)
            .earliest()
            .ok_or("invalid local time")?;
        if at > now {
            at
        } else {
            Local
                .from_local_datetime(&(today + chrono::Duration::days(1)))
                .earliest()
                .ok_or("invalid local time")?
        }
    } else {
        let n = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
            .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
            .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M"))
            .map_err(|_| "due must be YYYY-MM-DDTHH:MM".to_string())?;
        Local
            .from_local_datetime(&n)
            .earliest()
            .ok_or("invalid local time")?
    };
    Ok(local
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    #[test]
    fn url_validation() {
        assert!(validate_url("https://example.com/a?b=c").is_ok());
        assert!(validate_url("http://example.com").is_ok());
        assert!(validate_url("file:///C:/Windows/System32/cmd.exe").is_err());
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("ms-settings:privacy").is_err());
        assert!(validate_url("https://").is_err());
        assert!(validate_url("https://user@evil.com").is_err());
        assert!(validate_url("https://a.com/\" & calc").is_err());
    }

    #[test]
    fn search_urls() {
        assert_eq!(
            search_url("youtube", "how to cook spaghetti").unwrap(),
            "https://www.youtube.com/results?search_query=how+to+cook+spaghetti"
        );
        assert_eq!(
            search_url("amazon", "gift wraps").unwrap(),
            "https://www.amazon.in/s?k=gift+wraps"
        );
        assert_eq!(
            search_url("google", "c++ & rust").unwrap(),
            "https://www.google.com/search?q=c%2B%2B+%26+rust"
        );
        assert!(search_url("altavista", "x").is_err());
        assert!(search_url("google", "  ").is_err());
    }

    #[test]
    fn arg_limits() {
        let a = json!({"text": "x".repeat(10), "n": 5});
        assert!(str_arg(&a, "text", 5).is_err());
        assert!(str_arg(&a, "text", 20).is_ok());
        assert!(str_arg(&a, "missing", 20).is_err());
        assert!(opt_str_arg(&a, "n", 5).is_err());
        assert_eq!(opt_str_arg(&a, "missing", 5).unwrap(), None);
    }

    #[test]
    fn due_parsing() {
        let now = chrono::Local
            .with_ymd_and_hms(2026, 10, 12, 17, 0, 0)
            .unwrap();
        let today6 = parse_due("18:00", now).unwrap();
        let expect = chrono::Local
            .with_ymd_and_hms(2026, 10, 12, 18, 0, 0)
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            today6,
            expect.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );
        let late = chrono::Local
            .with_ymd_and_hms(2026, 10, 12, 19, 0, 0)
            .unwrap();
        let tomorrow = parse_due("18:00", late).unwrap();
        let expect = chrono::Local
            .with_ymd_and_hms(2026, 10, 13, 18, 0, 0)
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            tomorrow,
            expect.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );
        assert!(parse_due("2026-10-12T18:00", now).is_ok());
        assert!(parse_due("six pm", now).is_err());
    }

    #[test]
    fn tool_names_roundtrip() {
        for t in ToolName::ALL {
            assert_eq!(ToolName::parse(t.as_str()), Some(*t));
            assert_eq!(serde_json::to_value(t).unwrap(), json!(t.as_str()));
        }
    }
}
