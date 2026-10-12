//! Hidden window for system notifications: session lock/unlock (FR-SCH-08),
//! resume from sleep (NFR-32) and clipboard changes (FR-ISL-07).

use std::sync::OnceLock;

use tokio::sync::mpsc::UnboundedSender;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::AddClipboardFormatListener;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
    TranslateMessage, MSG, WINDOW_EX_STYLE, WM_CLIPBOARDUPDATE, WM_POWERBROADCAST,
    WM_WTSSESSION_CHANGE, WNDCLASSW, WS_OVERLAPPED,
};

const WTS_SESSION_LOCK: usize = 0x7;
const WTS_SESSION_UNLOCK: usize = 0x8;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
const PBT_APMRESUMESUSPEND: usize = 0x7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysEvent {
    Locked,
    Unlocked,
    Resumed,
    ClipboardChanged,
}

static TX: OnceLock<UnboundedSender<SysEvent>> = OnceLock::new();

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let send = |e| {
        if let Some(tx) = TX.get() {
            let _ = tx.send(e);
        }
    };
    match msg {
        WM_WTSSESSION_CHANGE => match wp.0 {
            WTS_SESSION_LOCK => send(SysEvent::Locked),
            WTS_SESSION_UNLOCK => send(SysEvent::Unlocked),
            _ => {}
        },
        WM_POWERBROADCAST => {
            if wp.0 == PBT_APMRESUMEAUTOMATIC || wp.0 == PBT_APMRESUMESUSPEND {
                send(SysEvent::Resumed);
            }
        }
        WM_CLIPBOARDUPDATE => send(SysEvent::ClipboardChanged),
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

pub fn start(tx: UnboundedSender<SysEvent>) {
    let _ = TX.set(tx);
    std::thread::Builder::new()
        .name("wisp-sysevents".into())
        .spawn(|| unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let class = w!("WispSysEvents");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            // A hidden top-level window (not message-only) so it receives broadcasts.
            let hwnd = match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("Wisp system events"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinst.into()),
                None,
            ) {
                Ok(h) => h,
                Err(e) => {
                    tracing::error!(error = %e, "system event window failed");
                    return;
                }
            };
            if WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION).is_err() {
                tracing::warn!("session notifications unavailable");
            }
            if AddClipboardFormatListener(hwnd).is_err() {
                tracing::warn!("clipboard listener unavailable");
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn sysevents");
}
