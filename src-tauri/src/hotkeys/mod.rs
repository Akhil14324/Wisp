//! Low-level keyboard hook on a dedicated thread (section 7.1, FR-HK-01 to 07).
//! The callbacks only update key state and push to a channel (FR-HK-04).

pub mod chord;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KillTimer, PostThreadMessageW, SetTimer,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_TIMER,
};

use crate::input::{self, WISP_MARKER};
pub use chord::{ChordAction, ChordDef, HookEvent};

const WM_RELOAD: u32 = WM_APP + 1;
const WM_REINSTALL: u32 = WM_APP + 2;

static SENDER: OnceLock<UnboundedSender<HookEvent>> = OnceLock::new();
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
static LAST_CALLBACK: AtomicU32 = AtomicU32::new(0);
static PAUSED: AtomicBool = AtomicBool::new(false);
static REPORT_PHYSICAL: AtomicBool = AtomicBool::new(false);
static PENDING_CHORDS: Mutex<Option<Vec<ChordDef>>> = Mutex::new(None);

thread_local! {
    static MACHINE: RefCell<chord::ChordMachine> = RefCell::new(chord::ChordMachine::new(vec![]));
    static TIMER: RefCell<usize> = const { RefCell::new(0) };
}

/// Builds chord definitions from the hotkey settings.
pub fn chords_from(dictation: &str, circle: &str, command: &str) -> Vec<ChordDef> {
    [
        (ChordAction::Dictation, dictation),
        (ChordAction::Circle, circle),
        (ChordAction::Command, command),
    ]
    .into_iter()
    .filter_map(|(a, s)| ChordDef::parse(a, s))
    .collect()
}

/// Starts the hook thread and its watchdog.
pub fn start(tx: UnboundedSender<HookEvent>, chords: Vec<ChordDef>) {
    let _ = SENDER.set(tx);
    *PENDING_CHORDS.lock().unwrap() = Some(chords);
    std::thread::Builder::new()
        .name("wisp-hook".into())
        .spawn(hook_thread)
        .expect("spawn hook thread");
    std::thread::Builder::new()
        .name("wisp-hook-watchdog".into())
        .spawn(watchdog)
        .expect("spawn watchdog");
}

/// Applies new chords (FR-HK-06).
pub fn reload(chords: Vec<ChordDef>) {
    *PENDING_CHORDS.lock().unwrap() = Some(chords);
    post(WM_RELOAD);
}

/// Pause hotkeys toggle (FR-HK-07).
pub fn set_paused(p: bool) {
    PAUSED.store(p, Ordering::Relaxed);
    post(WM_RELOAD);
}

pub fn is_paused() -> bool {
    PAUSED.load(Ordering::Relaxed)
}

/// Turns takeover detection on while an agent task runs (FR-CU-14).
pub fn set_report_physical(on: bool) {
    REPORT_PHYSICAL.store(on, Ordering::Relaxed);
}

/// Reinstalls the hook (watchdog, resume from sleep: NFR-32).
pub fn reinstall() {
    post(WM_REINSTALL);
}

pub fn stop() {
    post(WM_QUIT);
}

fn post(msg: u32) {
    let tid = THREAD_ID.load(Ordering::Relaxed);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, msg, WPARAM(0), LPARAM(0));
        }
    }
}

fn emit(e: HookEvent) {
    if let Some(tx) = SENDER.get() {
        let _ = tx.send(e);
    }
}

fn install() -> (HHOOK, HHOOK) {
    unsafe {
        let hmod: HINSTANCE = GetModuleHandleW(None).map(|h| h.into()).unwrap_or_default();
        let kb =
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), Some(hmod), 0).unwrap_or_default();
        let ms =
            SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(hmod), 0).unwrap_or_default();
        if kb.is_invalid() {
            tracing::error!("keyboard hook install failed");
        }
        LAST_CALLBACK.store(GetTickCount(), Ordering::Relaxed);
        (kb, ms)
    }
}

fn uninstall(h: (HHOOK, HHOOK)) {
    unsafe {
        if !h.0.is_invalid() {
            let _ = UnhookWindowsHookEx(h.0);
        }
        if !h.1.is_invalid() {
            let _ = UnhookWindowsHookEx(h.1);
        }
    }
}

fn apply_pending() {
    let pending = PENDING_CHORDS.lock().ok().and_then(|mut g| g.take());
    MACHINE.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(c) = pending {
            m.set_chords(c);
        }
        m.paused = PAUSED.load(Ordering::Relaxed);
    });
}

fn arm_timer(delay: Duration) {
    TIMER.with(|t| unsafe {
        let mut t = t.borrow_mut();
        if *t != 0 {
            let _ = KillTimer(None, *t);
        }
        *t = SetTimer(None, 0, delay.as_millis().max(10) as u32, None);
    });
}

fn hook_thread() {
    THREAD_ID.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    apply_pending();
    let mut hooks = install();
    tracing::info!("keyboard hook installed");
    let mut msg = MSG::default();
    loop {
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if r.0 <= 0 {
            break;
        }
        match msg.message {
            WM_TIMER => {
                TIMER.with(|t| unsafe {
                    let mut t = t.borrow_mut();
                    if *t != 0 {
                        let _ = KillTimer(None, *t);
                        *t = 0;
                    }
                });
                let (ev, again) = MACHINE.with(|m| m.borrow_mut().tick(Instant::now()));
                if let Some(e) = ev {
                    emit(e);
                }
                if let Some(d) = again {
                    arm_timer(d);
                }
            }
            WM_RELOAD => apply_pending(),
            WM_REINSTALL => {
                uninstall(hooks);
                hooks = install();
                tracing::warn!("keyboard hook reinstalled");
            }
            _ => unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            },
        }
    }
    uninstall(hooks);
    tracing::info!("keyboard hook removed");
}

unsafe extern "system" fn kb_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    LAST_CALLBACK.store(GetTickCount(), Ordering::Relaxed);
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    let injected = info.flags.0 & LLKHF_INJECTED.0 != 0;
    if injected && info.dwExtraInfo == WISP_MARKER {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let report = REPORT_PHYSICAL.load(Ordering::Relaxed);
    let decision = MACHINE.with(|m| {
        m.borrow_mut()
            .on_key(info.vkCode as u16, down, !injected, report, Instant::now())
    });
    for e in decision.events.iter().flatten() {
        emit(*e);
    }
    if decision.arm_timer {
        arm_timer(chord::ARM_DELAY);
    }
    if decision.mask_win {
        // Mask key first, then the Win release, so the Start menu stays closed.
        input::tap_mask_key();
        input::inject_key_up(info.vkCode as u16);
        return LRESULT(1);
    }
    if decision.swallow {
        return LRESULT(1);
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    LAST_CALLBACK.store(GetTickCount(), Ordering::Relaxed);
    if code >= 0 && REPORT_PHYSICAL.load(Ordering::Relaxed) {
        let msg = wparam.0 as u32;
        if matches!(msg, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            if info.flags & LLMHF_INJECTED == 0 {
                emit(HookEvent::PhysicalInput);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// FR-HK-05: if input happened more than 5 s after the last callback, the hook
/// was probably removed by Windows; reinstall it.
fn watchdog() {
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let mut lii = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if !unsafe { GetLastInputInfo(&mut lii) }.as_bool() {
            continue;
        }
        let last_cb = LAST_CALLBACK.load(Ordering::Relaxed);
        let gap = lii.dwTime.wrapping_sub(last_cb);
        if gap < u32::MAX / 2 && gap > 5000 {
            tracing::warn!(gap_ms = gap, "hook looks removed; reinstalling");
            LAST_CALLBACK.store(lii.dwTime, Ordering::Relaxed);
            reinstall();
        }
    }
}
