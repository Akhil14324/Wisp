//! Island state machine, window placement and events to the UI (section 4.9).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder,
};

use crate::capture;
use crate::error::ErrCode;
use crate::winutil;

pub const LABEL: &str = "island";
pub const PILL: (f64, f64) = (140.0, 34.0);
/// Fixed island window size (logical px): room for the largest panel or card.
pub const WINDOW: (f64, f64) = (460.0, 440.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Idle,
    Listening,
    Thinking,
    Acting,
    Confirm,
    Result,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct IslandEvent {
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    /// Computer Use banner is showing (FR-CU-02).
    pub controlling: bool,
}

pub struct Island {
    app: AppHandle,
    gen: AtomicU64,
    current: Mutex<State>,
    controlling: AtomicBool,
    size: Mutex<(f64, f64)>,
}

fn truncate(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

impl Island {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            gen: AtomicU64::new(0),
            current: Mutex::new(State::Idle),
            controlling: AtomicBool::new(false),
            size: Mutex::new(PILL),
        }
    }

    pub fn state(&self) -> State {
        *self.current.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn emit(&self, state: State, text: Option<String>, code: Option<&'static str>) -> u64 {
        let g = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        *self.current.lock().unwrap_or_else(|p| p.into_inner()) = state;
        let ev = IslandEvent {
            state,
            text,
            code,
            controlling: self.controlling.load(Ordering::SeqCst),
        };
        let _ = self.app.emit("wisp://island-state", ev);
        if state != State::Idle {
            self.show();
        }
        g
    }

    pub fn set(&self, state: State, text: Option<&str>) {
        self.emit(state, text.map(|t| t.to_string()), None);
    }

    pub fn idle(&self) {
        self.emit(State::Idle, None, None);
    }

    /// Acting text is the model's intent, truncated to 60 characters (section 8.4).
    pub fn acting(&self, intent: &str) {
        self.emit(State::Acting, Some(truncate(intent, 60)), None);
    }

    fn flash(&'static self, state: State, text: String, code: Option<&'static str>, dur: Duration) {
        let g = self.emit(state, Some(text), code);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(dur).await;
            if self.gen.load(Ordering::SeqCst) == g {
                self.idle();
            }
        });
    }

    /// Result text for up to 6 s (FR-CMD-03).
    pub fn result(&'static self, text: &str) {
        self.flash(
            State::Result,
            text.trim().to_string(),
            None,
            Duration::from_secs(6),
        );
    }

    /// Error text for 4 s (FR-ISL-02); "Didn't catch that" is 2 s (FR-DIC-05).
    pub fn error(&'static self, code: ErrCode) {
        let dur = if code == ErrCode::EmptyResult {
            Duration::from_secs(2)
        } else {
            Duration::from_secs(4)
        };
        self.flash(
            State::Error,
            code.island_message().to_string(),
            Some(code.as_str()),
            dur,
        );
    }

    pub fn error_text(&'static self, text: &str) {
        self.flash(
            State::Error,
            truncate(text, 120),
            None,
            Duration::from_secs(4),
        );
    }

    pub fn set_controlling(&self, on: bool) {
        self.controlling.store(on, Ordering::SeqCst);
        let st = self.state();
        self.emit(st, None, None);
    }

    pub fn window(&self) -> Option<tauri::WebviewWindow> {
        self.app.get_webview_window(LABEL)
    }

    /// Shows the island without stealing focus (FR-ISL-01).
    pub fn show(&self) {
        if let Some(w) = self.window() {
            if !w.is_visible().unwrap_or(true) {
                if let Ok(h) = w.hwnd() {
                    winutil::show_no_activate(h.0 as isize);
                }
            }
        }
    }

    /// Records the visible shape (logical px). The window itself keeps a fixed
    /// size so the UI can animate the shape smoothly; only the shape takes the
    /// mouse (FR-ISL-04, see `start_hit_test`).
    pub fn layout(&self, w: f64, h: f64) {
        let w = w.clamp(40.0, WINDOW.0);
        let h = h.clamp(20.0, WINDOW.1);
        *self.size.lock().unwrap_or_else(|p| p.into_inner()) = (w, h);
    }

    /// Visible shape size in logical px.
    pub fn shape(&self) -> (f64, f64) {
        *self.size.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Keeps the island window at top-centre of its monitor, 8 px down.
    pub fn place(&self, monitor_setting: &str) {
        let Some(win) = self.window() else { return };
        let Some(m) = capture::monitor_for_setting(monitor_setting) else {
            return;
        };
        let scale = m.scale.max(0.5) as f64;
        let pw = (WINDOW.0 * scale).round() as i32;
        let x = m.x + (m.width as i32 - pw) / 2;
        let y = m.y + (8.0 * scale).round() as i32;
        let _ = win.set_position(PhysicalPosition::new(x, y));
        let _ = win.set_size(LogicalSize::new(WINDOW.0, WINDOW.1));
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
}

/// While the typed box or a confirmation card is open, the whole shape must
/// keep the mouse even if the pointer strays.
static FORCE_HIT: AtomicBool = AtomicBool::new(false);

/// True when a physical point is inside the visible shape. The shape is
/// centred horizontally at the top of the window.
pub fn shape_contains(win: (i32, i32, i32), shape: (f64, f64), pt: (i32, i32)) -> bool {
    let (wx, wy, width_px) = win;
    let scale = width_px as f64 / WINDOW.0;
    let sw = shape.0 * scale;
    let sh = shape.1 * scale;
    let x0 = wx as f64 + (width_px as f64 - sw) / 2.0 - 2.0;
    let y0 = wy as f64 - 2.0;
    let (px, py) = (pt.0 as f64, pt.1 as f64);
    px >= x0 && px <= x0 + sw + 4.0 && py >= y0 && py <= y0 + sh + 4.0
}

/// Polls the cursor (~30 Hz) and makes the transparent part of the island
/// window click-through, so only the pill or panel takes the mouse.
pub fn start_hit_test(app: AppHandle) {
    std::thread::Builder::new()
        .name("wisp-island-hit".into())
        .spawn(move || {
            let mut inside_prev: Option<bool> = None;
            loop {
                std::thread::sleep(Duration::from_millis(33));
                let Some(c) = crate::state::try_ctx() else {
                    continue;
                };
                let Some(win) = app.get_webview_window(LABEL) else {
                    continue;
                };
                let Ok(h) = win.hwnd() else { continue };
                let Some(rect) = winutil::window_rect(h.0 as isize) else {
                    continue;
                };
                let pt = capture::cursor_pos();
                let inside = FORCE_HIT.load(Ordering::SeqCst)
                    || shape_contains((rect.0, rect.1, rect.2 - rect.0), c.island.shape(), pt);
                if inside_prev != Some(inside) {
                    let _ = win.set_ignore_cursor_events(!inside);
                    let _ = app.emit(
                        "wisp://island-hover",
                        serde_json::json!({ "inside": inside }),
                    );
                    inside_prev = Some(inside);
                }
            }
        })
        .expect("spawn island hit test");
}
/// Creates the island window (hidden until placed).
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let win =
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("src/island/index.html".into()))
            .title("Wisp")
            .inner_size(WINDOW.0, WINDOW.1)
            .transparent(true)
            .decorations(false)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(false)
            .visible(false)
            .build()?;
    if let Ok(h) = win.hwnd() {
        winutil::set_no_activate(h.0 as isize, true);
        winutil::exclude_from_capture(h.0 as isize);
    }
    Ok(())
}

static PREV_FOREGROUND: Mutex<Option<isize>> = Mutex::new(None);

/// Lets the island take keyboard focus for the typed command box (FR-CMD-04)
/// and the confirmation card, then hands focus back to the previous window.
pub fn set_interactive(app: &AppHandle, on: bool) {
    let Some(w) = app.get_webview_window(LABEL) else {
        return;
    };
    let Ok(h) = w.hwnd() else { return };
    let me = h.0 as isize;
    FORCE_HIT.store(on, Ordering::SeqCst);
    if on {
        let _ = w.set_ignore_cursor_events(false);
        let fg = winutil::foreground();
        if fg.hwnd != 0 && fg.hwnd != me {
            *PREV_FOREGROUND.lock().unwrap_or_else(|p| p.into_inner()) = Some(fg.hwnd);
        }
        winutil::set_no_activate(me, false);
        winutil::focus(me);
        let _ = w.set_focus();
    } else {
        winutil::set_no_activate(me, true);
        if let Some(prev) = PREV_FOREGROUND
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            winutil::focus(prev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_test_follows_centred_shape() {
        // 460 logical px window at 150% = 690 px wide, at x=615, y=12.
        let win = (615, 12, 690);
        let pill = (140.0, 34.0);
        // Centre of the pill.
        assert!(shape_contains(win, pill, (615 + 345, 12 + 25)));
        // Same height but far to the left: transparent area, click-through.
        assert!(!shape_contains(win, pill, (630, 30)));
        // Below the pill.
        assert!(!shape_contains(win, pill, (960, 12 + 120)));
        // The expanded panel covers that point.
        assert!(shape_contains(win, (420.0, 300.0), (960, 12 + 120)));
    }
}
