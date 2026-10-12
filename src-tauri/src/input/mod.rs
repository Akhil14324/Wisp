//! SendInput wrappers (section 7.4). Every key and button pressed here is
//! tracked so an abort or panic can release it (FR-CU-10, NFR-31).

pub mod keys;

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Marker in `dwExtraInfo` so our own hook ignores injected events (section 7.1).
pub const WISP_MARKER: usize = 0x5749_5350;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Right,
    Middle,
}

static HELD_KEYS: Mutex<Option<HashSet<u16>>> = Mutex::new(None);
static HELD_BUTTONS: Mutex<Option<HashSet<Button>>> = Mutex::new(None);

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("Windows blocked the input (the target may be elevated)")]
    Blocked,
    #[error("{0}")]
    Key(String),
}

fn is_extended(vk: u16) -> bool {
    matches!(
        vk,
        0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x5D | 0xA3 | 0xA5 | 0x6F | 0x90 | 0x2C
    )
}

fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: WISP_MARKER,
            },
        },
    }
}

fn mouse_input(dx: i32, dy: i32, data: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: WISP_MARKER,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), InputError> {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if (sent as usize) < inputs.len() {
        Err(InputError::Blocked)
    } else {
        Ok(())
    }
}

fn track_key(vk: u16, down: bool) {
    let mut g = HELD_KEYS.lock().unwrap_or_else(|p| p.into_inner());
    let set = g.get_or_insert_with(HashSet::new);
    if down {
        set.insert(vk);
    } else {
        set.remove(&vk);
    }
}

fn track_button(b: Button, down: bool) {
    let mut g = HELD_BUTTONS.lock().unwrap_or_else(|p| p.into_inner());
    let set = g.get_or_insert_with(HashSet::new);
    if down {
        set.insert(b);
    } else {
        set.remove(&b);
    }
}

pub fn key_down(vk: u16) -> Result<(), InputError> {
    let mut f = KEYBD_EVENT_FLAGS(0);
    if is_extended(vk) {
        f |= KEYEVENTF_EXTENDEDKEY;
    }
    send(&[key_input(vk, 0, f)])?;
    track_key(vk, true);
    Ok(())
}

pub fn key_up(vk: u16) -> Result<(), InputError> {
    let mut f = KEYEVENTF_KEYUP;
    if is_extended(vk) {
        f |= KEYEVENTF_EXTENDEDKEY;
    }
    let r = send(&[key_input(vk, 0, f)]);
    track_key(vk, false);
    r
}

/// Injects the unassigned mask key (FR-HK-03).
pub fn tap_mask_key() {
    let _ = send(&[
        key_input(keys::VK_MASK, 0, KEYBD_EVENT_FLAGS(0)),
        key_input(keys::VK_MASK, 0, KEYEVENTF_KEYUP),
    ]);
}

/// Re-injects a key release with the marker (used for the Win key, FR-HK-03).
pub fn inject_key_up(vk: u16) {
    let mut f = KEYEVENTF_KEYUP;
    if is_extended(vk) {
        f |= KEYEVENTF_EXTENDEDKEY;
    }
    let _ = send(&[key_input(vk, 0, f)]);
}

/// Modifiers down in order, key down and up, modifiers up in reverse.
pub fn press_chord(chord: &keys::Chord) -> Result<(), InputError> {
    let mut pressed = vec![];
    let mut result = Ok(());
    for vk in &chord.keys {
        if let Err(e) = key_down(*vk) {
            result = Err(e);
            break;
        }
        pressed.push(*vk);
        std::thread::sleep(Duration::from_millis(8));
    }
    for vk in pressed.iter().rev() {
        let _ = key_up(*vk);
        std::thread::sleep(Duration::from_millis(4));
    }
    result
}

pub fn press_keys(spec: &str) -> Result<(), InputError> {
    let chord = keys::parse_chord(spec).map_err(|e| InputError::Key(e.0))?;
    press_chord(&chord)
}

/// Types text as Unicode events, one UTF-16 unit per event, 5 ms apart.
pub fn type_text(
    text: &str,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<(), InputError> {
    for unit in text.encode_utf16() {
        if cancel.map(|c| c.is_cancelled()).unwrap_or(false) {
            break;
        }
        if unit == '\n' as u16 {
            press_chord(&keys::Chord {
                keys: vec![keys::VK_RETURN],
            })?;
        } else if unit != '\r' as u16 {
            send(&[
                key_input(0, unit, KEYEVENTF_UNICODE),
                key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ])?;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

fn virtual_desktop() -> (i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        )
    }
}

/// Physical pixel to the 0-65535 virtual-desktop range.
pub fn normalize(x: i32, y: i32, vd: (i32, i32, i32, i32)) -> (i32, i32) {
    let (vx, vy, vw, vh) = vd;
    let nx = (((x - vx) as i64 * 65535) / (vw - 1).max(1) as i64) as i32;
    let ny = (((y - vy) as i64 * 65535) / (vh - 1).max(1) as i64) as i32;
    (nx.clamp(0, 65535), ny.clamp(0, 65535))
}

const ABS: MOUSE_EVENT_FLAGS =
    MOUSE_EVENT_FLAGS(MOUSEEVENTF_ABSOLUTE.0 | MOUSEEVENTF_VIRTUALDESK.0);

pub fn move_to(x: i32, y: i32) -> Result<(), InputError> {
    let (nx, ny) = normalize(x, y, virtual_desktop());
    send(&[mouse_input(nx, ny, 0, MOUSEEVENTF_MOVE | ABS)])
}

fn button_flags(b: Button, down: bool) -> MOUSE_EVENT_FLAGS {
    match (b, down) {
        (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
        (Button::Left, false) => MOUSEEVENTF_LEFTUP,
        (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
        (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
        (Button::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
        (Button::Middle, false) => MOUSEEVENTF_MIDDLEUP,
    }
}

pub fn button(b: Button, down: bool, x: i32, y: i32) -> Result<(), InputError> {
    let (nx, ny) = normalize(x, y, virtual_desktop());
    let r = send(&[mouse_input(
        nx,
        ny,
        0,
        button_flags(b, down) | MOUSEEVENTF_MOVE | ABS,
    )]);
    track_button(b, down && r.is_ok());
    r
}

pub fn click(b: Button, x: i32, y: i32, count: u32) -> Result<(), InputError> {
    move_to(x, y)?;
    std::thread::sleep(Duration::from_millis(15));
    for i in 0..count {
        button(b, true, x, y)?;
        std::thread::sleep(Duration::from_millis(12));
        button(b, false, x, y)?;
        if i + 1 < count {
            std::thread::sleep(Duration::from_millis(60));
        }
    }
    Ok(())
}

/// Wheel scroll in multiples of 120.
pub fn scroll(x: i32, y: i32, direction: &str, notches: i32) -> Result<(), InputError> {
    move_to(x, y)?;
    let (flag, sign) = match direction {
        "up" => (MOUSEEVENTF_WHEEL, 1),
        "down" => (MOUSEEVENTF_WHEEL, -1),
        "left" => (MOUSEEVENTF_HWHEEL, -1),
        _ => (MOUSEEVENTF_HWHEEL, 1),
    };
    for _ in 0..notches.max(1) {
        send(&[mouse_input(0, 0, 120 * sign, flag)])?;
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

/// Button down, 10 or more interpolated moves 10 ms apart, button up.
pub fn drag(from: (i32, i32), to: (i32, i32)) -> Result<(), InputError> {
    move_to(from.0, from.1)?;
    std::thread::sleep(Duration::from_millis(20));
    button(Button::Left, true, from.0, from.1)?;
    let steps = 16;
    let mut res = Ok(());
    for i in 1..=steps {
        let x = from.0 + (to.0 - from.0) * i / steps;
        let y = from.1 + (to.1 - from.1) * i / steps;
        std::thread::sleep(Duration::from_millis(10));
        if let Err(e) = move_to(x, y) {
            res = Err(e);
            break;
        }
    }
    let _ = button(Button::Left, false, to.0, to.1);
    res
}

/// Releases every key and button pressed by Wisp (FR-CU-10, NFR-31).
pub fn release_all() {
    let keys: Vec<u16> = HELD_KEYS
        .lock()
        .map(|mut g| g.get_or_insert_with(HashSet::new).drain().collect())
        .unwrap_or_default();
    for vk in keys {
        let mut f = KEYEVENTF_KEYUP;
        if is_extended(vk) {
            f |= KEYEVENTF_EXTENDEDKEY;
        }
        let _ = send(&[key_input(vk, 0, f)]);
    }
    let buttons: Vec<Button> = HELD_BUTTONS
        .lock()
        .map(|mut g| g.get_or_insert_with(HashSet::new).drain().collect())
        .unwrap_or_default();
    for b in buttons {
        let _ = send(&[mouse_input(0, 0, 0, button_flags(b, false))]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_spans_virtual_desktop() {
        let vd = (-1920, 0, 3840, 1080);
        assert_eq!(normalize(-1920, 0, vd), (0, 0));
        assert_eq!(normalize(1919, 1079, vd), (65535, 65535));
        let (mx, _) = normalize(0, 0, vd);
        assert!((32760..=32780).contains(&mx), "got {mx}");
    }
}
