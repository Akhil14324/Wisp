//! Raw clipboard access (section 7.5) and the in-memory history (FR-ISL-07).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};

const CF_TEXT: u32 = 1;
const CF_OEMTEXT: u32 = 7;
const CF_UNICODETEXT: u32 = 13;
const CF_LOCALE: u32 = 16;

/// Set while Wisp writes the clipboard so the history listener skips it.
pub static SELF_WRITE: AtomicBool = AtomicBool::new(false);

struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Opens the clipboard, retrying 5 times 20 ms apart.
fn open() -> anyhow::Result<Guard> {
    for _ in 0..5 {
        if unsafe { OpenClipboard(Some(HWND::default())) }.is_ok() {
            return Ok(Guard);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    anyhow::bail!("clipboard busy")
}

fn registered(name: windows::core::PCWSTR) -> u32 {
    unsafe { RegisterClipboardFormatW(name) }
}

/// Formats that only mark text content (not data that would be lost).
fn text_like_formats() -> Vec<u32> {
    vec![
        CF_TEXT,
        CF_OEMTEXT,
        CF_UNICODETEXT,
        CF_LOCALE,
        registered(w!("ExcludeClipboardContentFromMonitorProcessing")),
        registered(w!("CanIncludeInClipboardHistory")),
        registered(w!("CanUploadToCloudClipboard")),
    ]
}

/// What is on the clipboard right now.
pub enum Contents {
    Empty,
    Text(String),
    /// Some non-text format is present; never overwrite it (section 7.5).
    NonText,
}

fn read_unicode_locked() -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
            return None;
        }
        let h = GetClipboardData(CF_UNICODETEXT).ok()?;
        let hg = HGLOBAL(h.0);
        let p = GlobalLock(hg) as *const u16;
        if p.is_null() {
            return None;
        }
        let max = GlobalSize(hg) / 2;
        let mut len = 0usize;
        while len < max && *p.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        let _ = GlobalUnlock(hg);
        Some(s)
    }
}

pub fn contents() -> anyhow::Result<Contents> {
    let _g = open()?;
    let allowed = text_like_formats();
    let mut any = false;
    let mut fmt = 0u32;
    loop {
        fmt = unsafe { EnumClipboardFormats(fmt) };
        if fmt == 0 {
            break;
        }
        any = true;
        if !allowed.contains(&fmt) {
            return Ok(Contents::NonText);
        }
    }
    if !any {
        return Ok(Contents::Empty);
    }
    Ok(read_unicode_locked()
        .map(Contents::Text)
        .unwrap_or(Contents::Empty))
}

pub fn read_text() -> anyhow::Result<Option<String>> {
    let _g = open()?;
    Ok(read_unicode_locked())
}

pub fn write_text(text: &str) -> anyhow::Result<()> {
    let _g = open()?;
    SELF_WRITE.store(true, Ordering::SeqCst);
    let res = (|| unsafe {
        EmptyClipboard()?;
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = wide.len() * 2;
        let hg = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
        let p = GlobalLock(hg) as *mut u16;
        if p.is_null() {
            anyhow::bail!("GlobalLock failed");
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
        let _ = GlobalUnlock(hg);
        // On success the system owns the memory.
        SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hg.0)))?;
        Ok(())
    })();
    res
}

/// Clears Wisp's own clipboard (used to restore an empty clipboard).
pub fn clear() -> anyhow::Result<()> {
    let _g = open()?;
    SELF_WRITE.store(true, Ordering::SeqCst);
    unsafe { EmptyClipboard()? };
    Ok(())
}

/// True when the source app asked to keep this out of clipboard history.
pub fn excluded_from_history() -> bool {
    let Ok(_g) = open() else { return true };
    unsafe {
        let exclude = registered(w!("ExcludeClipboardContentFromMonitorProcessing"));
        if IsClipboardFormatAvailable(exclude).is_ok() {
            return true;
        }
        let can = registered(w!("CanIncludeInClipboardHistory"));
        if IsClipboardFormatAvailable(can).is_ok() {
            if let Ok(h) = GetClipboardData(can) {
                let hg = HGLOBAL(h.0);
                let p = GlobalLock(hg) as *const u32;
                let v = if p.is_null() { 1 } else { *p };
                let _ = GlobalUnlock(hg);
                return v == 0;
            }
        }
    }
    false
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipItem {
    pub id: u64,
    pub text: String,
    pub at: String,
}

/// Last 20 text items, memory only, cleared on quit (FR-ISL-07).
#[derive(Default)]
pub struct History {
    items: Mutex<VecDeque<ClipItem>>,
    next: Mutex<u64>,
}

impl History {
    pub fn push(&self, text: String) {
        if text.trim().is_empty() || text.len() > 100_000 {
            return;
        }
        let mut items = self.items.lock().unwrap_or_else(|p| p.into_inner());
        if items.front().map(|i| i.text == text).unwrap_or(false) {
            return;
        }
        let mut n = self.next.lock().unwrap_or_else(|p| p.into_inner());
        *n += 1;
        items.push_front(ClipItem {
            id: *n,
            text,
            at: crate::store::now_iso(),
        });
        items.truncate(20);
    }

    pub fn list(&self) -> Vec<ClipItem> {
        self.items
            .lock()
            .map(|i| i.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn get(&self, id: u64) -> Option<String> {
        self.items
            .lock()
            .ok()?
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.text.clone())
    }

    pub fn clear(&self) {
        if let Ok(mut i) = self.items.lock() {
            i.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_caps_and_dedupes() {
        let h = History::default();
        for i in 0..25 {
            h.push(format!("item {i}"));
        }
        h.push("item 24".into());
        let l = h.list();
        assert_eq!(l.len(), 20);
        assert_eq!(l[0].text, "item 24");
        assert!(h.get(l[0].id).is_some());
    }
}
