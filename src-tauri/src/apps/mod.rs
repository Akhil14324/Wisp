//! App index and launch (section 7.6, FR-CMD-05).

pub mod matching;

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppEntry {
    /// Stable id: "lnk:<path>" or "aumid:<AppUserModelID>".
    pub id: String,
    pub name: String,
    pub kind: AppKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    Shortcut,
    Store,
}

#[derive(Default)]
pub struct AppIndex {
    entries: RwLock<Vec<AppEntry>>,
}

const SKIP_WORDS: &[&str] = &[
    "uninstall",
    "readme",
    "help",
    "documentation",
    "license",
    "release notes",
    "website",
];

impl AppIndex {
    pub fn entries(&self) -> Vec<AppEntry> {
        self.entries.read().map(|e| e.clone()).unwrap_or_default()
    }

    pub fn find(&self, id: &str) -> Option<AppEntry> {
        self.entries
            .read()
            .ok()?
            .iter()
            .find(|e| e.id == id)
            .cloned()
    }

    pub fn resolve(&self, query: &str) -> matching::Resolution<AppEntry> {
        let entries = self.entries();
        matching::resolve(query, &entries, |e| e.name.as_str())
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<AppEntry> {
        let entries = self.entries();
        if query.trim().is_empty() {
            let mut all = entries;
            all.sort_by_key(|a| a.name.to_lowercase());
            all.truncate(limit);
            return all;
        }
        let mut scored: Vec<(f64, AppEntry)> = entries
            .into_iter()
            .map(|e| (matching::score(query, &e.name), e))
            .filter(|(s, _)| *s > 0.3)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(limit).map(|(_, e)| e).collect()
    }

    /// Rebuilds the index (startup and every 30 min).
    pub fn refresh(&self) {
        let mut list = Vec::new();
        for dir in start_menu_dirs() {
            collect_lnk(&dir, &mut list, 0);
        }
        let mut names: std::collections::HashSet<String> =
            list.iter().map(|e| matching::normalize(&e.name)).collect();
        for e in store_apps() {
            if names.insert(matching::normalize(&e.name)) {
                list.push(e);
            }
        }
        tracing::info!(count = list.len(), "app index refreshed");
        if let Ok(mut g) = self.entries.write() {
            *g = list;
        }
    }
}

fn start_menu_dirs() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Ok(pd) = std::env::var("ProgramData") {
        v.push(Path::new(&pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(ad) = std::env::var("APPDATA") {
        v.push(Path::new(&ad).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    v
}

fn collect_lnk(dir: &Path, out: &mut Vec<AppEntry>, depth: u32) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_lnk(&p, out, depth + 1);
        } else if p
            .extension()
            .map(|x| x.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false)
        {
            let name = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let lower = name.to_lowercase();
            if name.is_empty() || SKIP_WORDS.iter().any(|w| lower.contains(w)) {
                continue;
            }
            out.push(AppEntry {
                id: format!("lnk:{}", p.display()),
                name,
                kind: AppKind::Shortcut,
            });
        }
    }
}

#[derive(Deserialize)]
struct StartApp {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "AppID")]
    app_id: String,
}

/// Store apps via `Get-StartApps`, run by Wisp itself (the documented fallback
/// in section 7.6; the model can never trigger this).
fn store_apps() -> Vec<AppEntry> {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-StartApps | ConvertTo-Json -Compress",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let Ok(out) = out else { return vec![] };
    let text = String::from_utf8_lossy(&out.stdout);
    let apps: Vec<StartApp> = match serde_json::from_str::<serde_json::Value>(text.trim()) {
        Ok(serde_json::Value::Array(a)) => a
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
        Ok(v @ serde_json::Value::Object(_)) => {
            serde_json::from_value(v).ok().into_iter().collect()
        }
        _ => vec![],
    };
    apps.into_iter()
        // Packaged apps have AUMIDs of the form Family!App; desktop apps are covered by .lnk files.
        .filter(|a| a.app_id.contains('!') && !a.name.trim().is_empty())
        .filter(|a| {
            a.app_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-!".contains(c))
        })
        .map(|a| AppEntry {
            id: format!("aumid:{}", a.app_id),
            name: a.name,
            kind: AppKind::Store,
        })
        .collect()
}

fn shell_open(target: &str) -> anyhow::Result<()> {
    let r = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &HSTRING::from(target),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if r.0 as isize <= 32 {
        anyhow::bail!("ShellExecute failed ({})", r.0 as isize);
    }
    Ok(())
}

pub fn launch(entry: &AppEntry) -> anyhow::Result<()> {
    if let Some(path) = entry.id.strip_prefix("lnk:") {
        shell_open(path)
    } else if let Some(aumid) = entry.id.strip_prefix("aumid:") {
        std::process::Command::new("explorer.exe")
            .arg(format!("shell:AppsFolder\\{aumid}"))
            .spawn()?;
        Ok(())
    } else {
        anyhow::bail!("unknown app id")
    }
}

/// Opens an http(s) URL in the default browser (FR-CMD-06, NFR-13).
pub fn open_url(url: &str) -> anyhow::Result<()> {
    let checked = crate::tools::validate_url(url).map_err(|e| anyhow::anyhow!(e))?;
    shell_open(&checked)
}
