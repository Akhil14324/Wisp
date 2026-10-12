//! Key-name allowlist mapped to virtual-key codes (section 7.4).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyError(pub String);

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_MENU: u16 = 0x12;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_RWIN: u16 = 0x5C;
pub const VK_LSHIFT: u16 = 0xA0;
pub const VK_RSHIFT: u16 = 0xA1;
pub const VK_LCONTROL: u16 = 0xA2;
pub const VK_RCONTROL: u16 = 0xA3;
pub const VK_LMENU: u16 = 0xA4;
pub const VK_RMENU: u16 = 0xA5;
pub const VK_SPACE: u16 = 0x20;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_DELETE: u16 = 0x2E;
pub const VK_L: u16 = 0x4C;
pub const VK_F4: u16 = 0x73;
/// Unassigned key injected to stop the Start menu opening (FR-HK-03).
pub const VK_MASK: u16 = 0xE8;

pub fn is_modifier(vk: u16) -> bool {
    matches!(
        vk,
        VK_SHIFT
            | VK_CONTROL
            | VK_MENU
            | VK_LWIN
            | VK_RWIN
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_LMENU
            | VK_RMENU
    )
}

/// Maps one key name (case-insensitive; Playwright and common spellings) to a VK code.
pub fn vk_for(name: &str) -> Option<u16> {
    let n = name.trim().to_ascii_lowercase();
    let vk = match n.as_str() {
        "ctrl" | "control" | "controlleft" | "ctrlleft" => VK_CONTROL,
        "shift" | "shiftleft" => VK_SHIFT,
        "alt" | "option" | "altleft" | "menu" => VK_MENU,
        "win" | "windows" | "meta" | "super" | "cmd" | "command" | "os" | "metaleft" => VK_LWIN,
        "enter" | "return" => VK_RETURN,
        "tab" => 0x09,
        "esc" | "escape" => VK_ESCAPE,
        "space" | " " | "spacebar" => VK_SPACE,
        "backspace" | "back" => 0x08,
        "delete" | "del" => VK_DELETE,
        "insert" | "ins" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "left" | "arrowleft" => 0x25,
        "up" | "arrowup" => 0x26,
        "right" | "arrowright" => 0x27,
        "down" | "arrowdown" => 0x28,
        "capslock" => 0x14,
        "printscreen" | "prtsc" => 0x2C,
        "contextmenu" | "apps" => 0x5D,
        "volumemute" | "audiovolumemute" => 0xAD,
        "volumedown" | "audiovolumedown" => 0xAE,
        "volumeup" | "audiovolumeup" => 0xAF,
        "medianext" | "mediatracknext" => 0xB0,
        "mediaprevious" | "mediatrackprevious" => 0xB1,
        "mediaplaypause" => 0xB3,
        "-" | "minus" => 0xBD,
        "=" | "+" | "equal" | "plus" => 0xBB,
        "," | "comma" => 0xBC,
        "." | "period" => 0xBE,
        "/" | "slash" => 0xBF,
        ";" | "semicolon" => 0xBA,
        "'" | "quote" => 0xDE,
        "[" | "bracketleft" => 0xDB,
        "]" | "bracketright" => 0xDD,
        "\\" | "backslash" => 0xDC,
        "`" | "backquote" => 0xC0,
        _ => {
            let b = n.as_bytes();
            if b.len() == 1 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
                return Some(b[0].to_ascii_uppercase() as u16);
            }
            if let Some(rest) = n.strip_prefix("key") {
                if rest.len() == 1 {
                    return vk_for(rest);
                }
            }
            if let Some(rest) = n.strip_prefix("digit") {
                if rest.len() == 1 {
                    return vk_for(rest);
                }
            }
            if let Some(num) = n.strip_prefix('f').and_then(|s| s.parse::<u16>().ok()) {
                if (1..=24).contains(&num) {
                    return Some(0x70 + num - 1);
                }
            }
            return None;
        }
    };
    Some(vk)
}

/// A parsed chord: modifiers then main keys, in press order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub keys: Vec<u16>,
}

impl Chord {
    pub fn has(&self, vk: u16) -> bool {
        self.keys.contains(&vk)
    }
}

/// Parses "ctrl+shift+t" or "Control+A" style chords against the allowlist.
pub fn parse_chord(s: &str) -> Result<Chord, KeyError> {
    let s = s.trim();
    if s.is_empty() || s.len() > 64 {
        return Err(KeyError("empty or too long key chord".into()));
    }
    // "+" alone or "ctrl++" means the plus key.
    let mut parts: Vec<String> = vec![];
    for (i, p) in s.split('+').enumerate() {
        if p.is_empty() {
            if i > 0 && parts.last().map(|l: &String| l != "+").unwrap_or(true) {
                parts.push("+".into());
            }
            continue;
        }
        parts.push(p.to_string());
    }
    let mut keys = vec![];
    for p in &parts {
        let vk = vk_for(p).ok_or_else(|| KeyError(format!("key '{p}' is not allowed")))?;
        if !keys.contains(&vk) {
            keys.push(vk);
        }
    }
    if keys.is_empty() || keys.len() > 5 {
        return Err(KeyError("a chord has 1 to 5 keys".into()));
    }
    // Modifiers first so they go down before the main key.
    keys.sort_by_key(|k| !is_modifier(*k));
    Ok(Chord { keys })
}

/// Chords the press_keys tool may never send (section 4.4).
pub fn is_blocked(chord: &Chord, wisp_window_focused: bool) -> bool {
    let win = chord.has(VK_LWIN);
    let ctrl = chord.has(VK_CONTROL);
    let alt = chord.has(VK_MENU);
    (win && chord.has(VK_L))
        || (ctrl && alt && chord.has(VK_DELETE))
        || (alt && chord.has(VK_F4) && wisp_window_focused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_chords() {
        assert_eq!(parse_chord("ctrl+s").unwrap().keys, vec![VK_CONTROL, 0x53]);
        assert_eq!(
            parse_chord("Control+A").unwrap().keys,
            vec![VK_CONTROL, 0x41]
        );
        assert_eq!(parse_chord("s+ctrl").unwrap().keys, vec![VK_CONTROL, 0x53]);
        assert_eq!(parse_chord("F5").unwrap().keys, vec![0x74]);
        assert_eq!(parse_chord("ctrl++").unwrap().keys, vec![VK_CONTROL, 0xBB]);
        assert_eq!(parse_chord("Enter").unwrap().keys, vec![VK_RETURN]);
        assert_eq!(parse_chord("KeyA").unwrap().keys, vec![0x41]);
        assert!(parse_chord("ctrl+nuke").is_err());
        assert!(parse_chord("").is_err());
    }

    #[test]
    fn blocked_chords() {
        assert!(is_blocked(&parse_chord("win+l").unwrap(), false));
        assert!(is_blocked(&parse_chord("ctrl+alt+delete").unwrap(), false));
        assert!(is_blocked(&parse_chord("alt+f4").unwrap(), true));
        assert!(!is_blocked(&parse_chord("alt+f4").unwrap(), false));
        assert!(!is_blocked(&parse_chord("ctrl+s").unwrap(), false));
    }
}
