//! Small Win32 helpers: foreground window, elevation, window styles,
//! full-screen state (section 7.3, 7.8).

use std::time::{Duration, Instant};

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId,
    SetForegroundWindow, SetWindowDisplayAffinity, SetWindowLongPtrW, SystemParametersInfoW,
    GWL_EXSTYLE, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WDA_EXCLUDEFROMCAPTURE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::input::keys::{
    VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
};

#[derive(Debug, Clone, Default)]
pub struct Foreground {
    pub hwnd: isize,
    pub pid: u32,
    pub process: String,
    pub title: String,
    pub elevated: bool,
    pub is_wisp: bool,
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Reads TokenElevation; if the process cannot be opened, assume elevated (section 7.8).
fn is_elevated(pid: u32) -> (bool, String) {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return (true, String::new());
        };
        let h = Handle(h);
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let name = if QueryFullProcessImageNameW(
            h.0,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok()
        {
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit('\\').next().unwrap_or("").to_string()
        } else {
            String::new()
        };
        let mut tok = HANDLE::default();
        if OpenProcessToken(h.0, TOKEN_QUERY, &mut tok).is_err() {
            return (true, name);
        }
        let tok = Handle(tok);
        let mut elev = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            tok.0,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        );
        if ok.is_err() {
            return (true, name);
        }
        (elev.TokenIsElevated != 0, name)
    }
}

pub fn foreground() -> Foreground {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Foreground::default();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut tbuf = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut tbuf);
        let title = String::from_utf16_lossy(&tbuf[..n.max(0) as usize]);
        let is_wisp = pid == GetCurrentProcessId();
        let (elevated, process) = if is_wisp {
            (false, "wisp.exe".into())
        } else {
            is_elevated(pid)
        };
        Foreground {
            hwnd: hwnd.0 as isize,
            pid,
            process,
            title,
            elevated,
            is_wisp,
        }
    }
}

/// Island style: never activates, no taskbar button (FR-ISL-01).
pub fn set_no_activate(hwnd: isize, on: bool) {
    unsafe {
        let h = HWND(hwnd as *mut _);
        let mut ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        if on {
            ex |= (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        } else {
            ex &= !(WS_EX_NOACTIVATE.0 as isize);
        }
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex);
        let off: i32 = 1;
        let _ = DwmSetWindowAttribute(
            h,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &off as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// Window rectangle in physical pixels (left, top, right, bottom), if visible.
pub fn window_rect(hwnd: isize) -> Option<(i32, i32, i32, i32)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};
    unsafe {
        let h = HWND(hwnd as *mut _);
        if !IsWindowVisible(h).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(h, &mut r).ok()?;
        Some((r.left, r.top, r.right, r.bottom))
    }
}

/// Shows a topmost window without taking focus from the app in front.
pub fn show_no_activate(hwnd: isize) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_SHOWWINDOW, SW_SHOWNOACTIVATE,
    };
    unsafe {
        let h = HWND(hwnd as *mut _);
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
    }
}

pub fn focus(hwnd: isize) {
    unsafe {
        let _ = SetForegroundWindow(HWND(hwnd as *mut _));
    }
}

/// Keeps Wisp's own windows out of screenshots (section 7.3).
pub fn exclude_from_capture(hwnd: isize) {
    // Dev builds only: WISP_SHOW_IN_CAPTURE=1 keeps windows visible in screenshots.
    if cfg!(debug_assertions) && std::env::var_os("WISP_SHOW_IN_CAPTURE").is_some() {
        return;
    }
    unsafe {
        if SetWindowDisplayAffinity(HWND(hwnd as *mut _), WDA_EXCLUDEFROMCAPTURE).is_err() {
            tracing::warn!("SetWindowDisplayAffinity failed");
        }
    }
}

/// Full-screen app or presentation in front (FR-ISL-11).
pub fn fullscreen_busy() -> bool {
    unsafe {
        match SHQueryUserNotificationState() {
            Ok(s) => {
                s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE
            }
            Err(_) => false,
        }
    }
}

/// Windows "Show animations" setting (FR-ISL-12).
pub fn animations_enabled() -> bool {
    let mut on: i32 = 1;
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    on != 0
}

fn key_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

/// Waits (up to `max`) until the user has let go of all modifiers, so pasted
/// or typed text is not turned into shortcuts (e.g. Win+V).
pub fn wait_modifiers_released(max: Duration) {
    let start = Instant::now();
    let mods = [
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
    ];
    while start.elapsed() < max {
        if !mods.iter().any(|m| key_down(*m)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// Windows build number for diagnostics.
pub fn windows_build() -> String {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd")
        .args(["/C", "ver"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}
