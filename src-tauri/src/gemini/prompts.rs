//! Prompts (section 5.3). The single place to tune model behaviour.

pub const DICTATION_TEMPLATE: &str = "You turn spoken audio into clean written text.
Output ONLY the text the speaker meant to write. No quotes, no commentary.
Remove filler words and false starts. If the speaker corrects themselves,
keep only the corrected version. Add punctuation and capitalization.
Keep the speaker's wording, meaning and language. Do not add content.
Use these spellings when they occur: {dictionary}.
If the audio has no intelligible speech, output exactly: <EMPTY>";

pub const COMMAND_TEMPLATE: &str =
    "You are {assistant_name}, a voice assistant that controls a Windows 11 PC.
Tone: {tone}. {soul_addendum}
Now: {iso_datetime} ({timezone}). Resolve relative dates and times from this.
Use the provided tools to act. Never say an action succeeded unless a tool
result says so. Prefer one tool call per need. Keep replies under 20 words.
For tasks that need seeing or clicking the screen, call run_computer_task.
If the request is unclear and a wrong guess would be costly, ask one short
question.";

pub const CIRCLE_TO_ASK: &str =
    "The user circled part of their screen (red strokes in the first image; the
second image is a close crop). The audio is their question about it.
Answer concisely in Markdown, using what is visible. If the audio is unclear
or the crop does not contain what they ask about, say so briefly.";

/// Used when Circle to Ask has no strokes, so there is only one image.
pub const CIRCLE_TO_ASK_NO_STROKES: &str =
    "The image is the user's screen. The audio (or text) is their question about it.
Answer concisely in Markdown, using what is visible. If the question is unclear
or the screen does not contain what they ask about, say so briefly.";

/// Used by the describe_screen tool.
pub const DESCRIBE_SCREEN: &str =
    "The image is the user's current screen. Answer the question about it
concisely in plain text, using only what is visible.";

/// Computer Use system instruction (desktop environment).
pub const COMPUTER_USE: &str =
    "You are operating a Windows 11 desktop for the user with mouse and keyboard.
Complete the goal step by step. Prefer keyboard shortcuts that work on Windows
(Control, not Meta, for copy, paste and select all). When the goal is done,
reply with a one-line summary of what you did. Never enter passwords, payment
details or personal data unless the user's goal explicitly says to.";

pub const EMPTY_MARKER: &str = "<EMPTY>";

/// Builds the dictation system instruction with the personal dictionary (FR-DIC-08).
pub fn dictation(dictionary: &[String]) -> String {
    let dict = if dictionary.is_empty() {
        "(none)".to_string()
    } else {
        dictionary
            .iter()
            .take(200)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    };
    DICTATION_TEMPLATE.replace("{dictionary}", &dict)
}

pub struct CommandContext<'a> {
    pub assistant_name: &'a str,
    pub tone: &'a str,
    pub language: &'a str,
    pub addendum: &'a str,
    pub iso_datetime: &'a str,
    pub timezone: &'a str,
}

/// Builds the command-mode system instruction from the active Soul.
pub fn command(ctx: &CommandContext) -> String {
    let mut addendum = String::new();
    if !ctx.language.is_empty() && !ctx.language.eq_ignore_ascii_case("english") {
        addendum.push_str(&format!("Reply in {}. ", ctx.language));
    }
    addendum.push_str(&ctx.addendum.chars().take(500).collect::<String>());
    COMMAND_TEMPLATE
        .replace("{assistant_name}", ctx.assistant_name)
        .replace("{tone}", ctx.tone)
        .replace("{soul_addendum}", addendum.trim())
        .replace("{iso_datetime}", ctx.iso_datetime)
        .replace("{timezone}", ctx.timezone)
}

/// Appends the Soul addendum to an answer prompt (Circle to Ask). Never used for dictation.
pub fn with_soul(base: &str, addendum: &str) -> String {
    let a: String = addendum.chars().take(500).collect();
    if a.trim().is_empty() {
        base.to_string()
    } else {
        format!("{base}\n{}", a.trim())
    }
}

/// Interprets a dictation response: `None` means "didn't catch that" (FR-DIC-05).
pub fn clean_dictation(text: &str) -> Option<String> {
    let t = text.trim();
    let t = t
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(t)
        .trim();
    if t.is_empty() || t.contains(EMPTY_MARKER) {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictation_includes_dictionary() {
        let p = dictation(&["Akhil".into(), "Tauri".into()]);
        assert!(p.contains("Use these spellings when they occur: Akhil, Tauri."));
        assert!(dictation(&[]).contains("(none)"));
    }

    #[test]
    fn command_fills_all_placeholders() {
        let p = command(&CommandContext {
            assistant_name: "Wisp",
            tone: "calm",
            language: "English",
            addendum: "Be brief.",
            iso_datetime: "2026-10-12T09:00:00+05:30",
            timezone: "Asia/Kolkata",
        });
        assert!(!p.contains('{'));
        assert!(p.starts_with("You are Wisp,"));
        assert!(p.contains("Tone: calm. Be brief."));
        assert!(p.contains("Asia/Kolkata"));
    }

    #[test]
    fn empty_marker_and_quotes() {
        assert_eq!(clean_dictation("<EMPTY>"), None);
        assert_eq!(clean_dictation("  "), None);
        assert_eq!(
            clean_dictation("\"Meet at ten.\""),
            Some("Meet at ten.".into())
        );
    }
}
