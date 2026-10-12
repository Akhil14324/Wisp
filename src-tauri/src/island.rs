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
const MAX_W: f64 = 460.0;
const MAX_H: f64 = 420.0;

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

    /// Resizes the island and keeps it at top-centre of its monitor.
    pub fn layout(&self, w: f64, h: f64, monitor_setting: &str) {
        let w = w.clamp(PILL.0 - 40.0, MAX_W);
        let h = h.clamp(28.0, MAX_H);
        *self.size.lock().unwrap_or_else(|p| p.into_inner()) = (w, h);
        self.place(monitor_setting);
    }

    pub fn place(&self, monitor_setting: &str) {
        let Some(win) = self.window() else { return };
        let Some(m) = capture::monitor_for_setting(monitor_setting) else {
            return;
        };
        let (w, h) = *self.size.lock().unwrap_or_else(|p| p.into_inner());
        let scale = m.scale.max(0.5) as f64;
        let pw = (w * scale).round() as i32;
        let x = m.x + (m.width as i32 - pw) / 2;
        let y = m.y + (8.0 * scale).round() as i32;
        let _ = win.set_position(PhysicalPosition::new(x, y));
        let _ = win.set_size(LogicalSize::new(w, h));
    }
}

/// Creates the island window (hidden until placed).
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let win =
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("src/island/index.html".into()))
            .title("Wisp")
            .inner_size(PILL.0, PILL.1)
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
    if on {
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
