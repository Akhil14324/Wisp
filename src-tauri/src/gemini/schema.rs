//! Function declarations for the local tools (section 4.4).
//! Uses the generateContent `parameters` Schema (OpenAPI subset, upper-case types).

use serde_json::{json, Value};

use crate::tools::ToolName;

fn when_schema() -> Value {
    json!({
        "type": "OBJECT",
        "description": "When to run. Required fields by kind: once needs date and time; daily and weekdays need time; weekly needs days and time; monthly needs day_of_month and time; cron needs cron.",
        "properties": {
            "kind": { "type": "STRING", "enum": ["once", "daily", "weekdays", "weekly", "monthly", "cron"] },
            "date": { "type": "STRING", "description": "YYYY-MM-DD, local date" },
            "time": { "type": "STRING", "description": "HH:MM, 24-hour local time" },
            "days": { "type": "ARRAY", "items": { "type": "STRING", "enum": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] } },
            "day_of_month": { "type": "INTEGER", "description": "1 to 31" },
            "cron": { "type": "STRING", "description": "5-field cron expression, local time" }
        },
        "required": ["kind"]
    })
}

/// The declaration for one tool.
pub fn declaration(t: ToolName) -> Value {
    match t {
        ToolName::OpenApp => json!({
            "name": "open_app",
            "description": "Launch an installed Windows app by name, e.g. Spotify, Notepad, Unity Hub.",
            "parameters": { "type": "OBJECT", "properties": { "name": { "type": "STRING" } }, "required": ["name"] }
        }),
        ToolName::OpenUrl => json!({
            "name": "open_url",
            "description": "Open an http or https URL in the default browser.",
            "parameters": { "type": "OBJECT", "properties": { "url": { "type": "STRING" } }, "required": ["url"] }
        }),
        ToolName::WebSearch => json!({
            "name": "web_search",
            "description": "Open a search results page in the default browser. Engine defaults to the user's default.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "query": { "type": "STRING" },
                    "engine": { "type": "STRING", "enum": ["google", "youtube", "amazon", "wikipedia", "bing", "duckduckgo", "github", "maps"] }
                },
                "required": ["query"]
            }
        }),
        ToolName::TypeText => json!({
            "name": "type_text",
            "description": "Insert text into the currently focused window.",
            "parameters": { "type": "OBJECT", "properties": { "text": { "type": "STRING" } }, "required": ["text"] }
        }),
        ToolName::PressKeys => json!({
            "name": "press_keys",
            "description": "Send a key chord to the focused window, e.g. ctrl+s, alt+tab, enter, ctrl+shift+t.",
            "parameters": { "type": "OBJECT", "properties": { "keys": { "type": "STRING" } }, "required": ["keys"] }
        }),
        ToolName::ClipboardRead => json!({
            "name": "clipboard_read",
            "description": "Read the text currently on the clipboard.",
            "parameters": { "type": "OBJECT", "properties": {} }
        }),
        ToolName::ClipboardWrite => json!({
            "name": "clipboard_write",
            "description": "Put text on the clipboard.",
            "parameters": { "type": "OBJECT", "properties": { "text": { "type": "STRING" } }, "required": ["text"] }
        }),
        ToolName::MediaControl => json!({
            "name": "media_control",
            "description": "Control the current media session (music or video).",
            "parameters": {
                "type": "OBJECT",
                "properties": { "action": { "type": "STRING", "enum": ["play_pause", "next", "previous"] } },
                "required": ["action"]
            }
        }),
        ToolName::SetVolume => json!({
            "name": "set_volume",
            "description": "Set the speaker volume (0 to 100) or mute/unmute it. Give level, mute, or both.",
            "parameters": {
                "type": "OBJECT",
                "properties": { "level": { "type": "INTEGER" }, "mute": { "type": "BOOLEAN" } }
            }
        }),
        ToolName::CreateTodo => json!({
            "name": "create_todo",
            "description": "Add an item to the user's to-do list. If a time is spoken, set due as local YYYY-MM-DDTHH:MM (today if still ahead, else tomorrow).",
            "parameters": {
                "type": "OBJECT",
                "properties": { "title": { "type": "STRING" }, "due": { "type": "STRING", "description": "YYYY-MM-DDTHH:MM local" } },
                "required": ["title"]
            }
        }),
        ToolName::CreateSchedule => json!({
            "name": "create_schedule",
            "description": "Schedule a natural-language command to run later, once or repeatedly. The command is what Wisp will do at that time, e.g. 'Search Amazon for gift wraps'.",
            "parameters": {
                "type": "OBJECT",
                "properties": { "command": { "type": "STRING" }, "when": when_schema(), "name": { "type": "STRING" } },
                "required": ["command", "when"]
            }
        }),
        ToolName::CreateRoutine => json!({
            "name": "create_routine",
            "description": "Create a named recurring routine, e.g. every morning at 9 open Unity and Spotify.",
            "parameters": {
                "type": "OBJECT",
                "properties": { "name": { "type": "STRING" }, "command": { "type": "STRING" }, "when": when_schema() },
                "required": ["name", "command", "when"]
            }
        }),
        ToolName::SetWallpaper => json!({
            "name": "set_wallpaper",
            "description": "Set the desktop wallpaper from a local image file path.",
            "parameters": { "type": "OBJECT", "properties": { "path": { "type": "STRING" } }, "required": ["path"] }
        }),
        ToolName::DescribeScreen => json!({
            "name": "describe_screen",
            "description": "Look at the user's screen and answer a question about what is visible.",
            "parameters": { "type": "OBJECT", "properties": { "question": { "type": "STRING" } }, "required": ["question"] }
        }),
        ToolName::RunComputerTask => json!({
            "name": "run_computer_task",
            "description": "Operate the PC with mouse and keyboard to complete a multi-step goal that needs seeing or clicking the screen.",
            "parameters": { "type": "OBJECT", "properties": { "goal": { "type": "STRING" } }, "required": ["goal"] }
        }),
    }
}

/// The `tools` array for a generateContent request (FR-CMD-07). Empty when no tools.
pub fn tools_field(enabled: &[ToolName]) -> Option<Value> {
    if enabled.is_empty() {
        return None;
    }
    let decls: Vec<Value> = ToolName::ALL
        .iter()
        .filter(|t| enabled.contains(t))
        .map(|t| declaration(*t))
        .collect();
    Some(json!([{ "functionDeclarations": decls }]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_enum() {
        for t in ToolName::ALL {
            assert_eq!(declaration(*t)["name"].as_str().unwrap(), t.as_str());
        }
    }

    #[test]
    fn only_enabled_tools_declared() {
        let v = tools_field(&[ToolName::OpenApp, ToolName::WebSearch]).unwrap();
        let decls = v[0]["functionDeclarations"].as_array().unwrap();
        assert_eq!(decls.len(), 2);
        assert!(tools_field(&[]).is_none());
    }
}
