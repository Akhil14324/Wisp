//! Process-wide state. One instance, created at startup and never dropped.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};

use tauri::{AppHandle, Emitter};

use crate::agent::AgentControl;
use crate::apps::AppIndex;
use crate::audio::Recorder;
use crate::clipboard;
use crate::cta::CtaState;
use crate::gemini::client::ModelInfo;
use crate::gemini::GeminiClient;
use crate::island::Island;
use crate::scheduler::Scheduler;
use crate::secrets::{self, ApiKey};
use crate::store::{self, settings::Settings, Db};

pub struct AppState {
    pub app: AppHandle,
    pub db: Db,
    pub settings: RwLock<Settings>,
    pub gemini: GeminiClient,
    pub recorder: Recorder,
    pub apps: AppIndex,
    pub clip_history: clipboard::History,
    pub island: Island,
    pub agent: AgentControl,
    pub scheduler: Scheduler,
    pub cta: CtaState,
    pub session_locked: AtomicBool,
    pub models_cache: Mutex<Option<Vec<ModelInfo>>>,
    /// Which chord is currently recording, so ChordUp matches ChordDown.
    pub recording_for: Mutex<Option<crate::hotkeys::ChordAction>>,
}

static STATE: OnceLock<AppState> = OnceLock::new();

pub fn init(s: AppState) -> &'static AppState {
    if STATE.set(s).is_err() {
        panic!("state initialised twice");
    }
    ctx()
}

/// The global state. Only valid after setup.
pub fn ctx() -> &'static AppState {
    STATE.get().expect("state not initialised")
}

pub fn try_ctx() -> Option<&'static AppState> {
    STATE.get()
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn reload_settings(&self) {
        let s = store::settings::load(&self.db);
        if let Ok(mut g) = self.settings.write() {
            *g = s;
        }
    }

    pub fn key(&self) -> Option<ApiKey> {
        secrets::get()
    }

    pub fn emit<S: serde::Serialize + Clone>(&self, topic: &str, payload: S) {
        let _ = self.app.emit(&format!("wisp://{topic}"), payload);
    }

    pub fn is_locked(&self) -> bool {
        self.session_locked.load(Ordering::SeqCst)
    }

    /// Active Soul preset (falls back to defaults).
    pub fn soul(&self) -> store::presets::Soul {
        store::presets::active(&self.db, "soul")
            .and_then(|p| serde_json::from_value(p.data).ok())
            .unwrap_or(store::presets::Soul {
                name: "Wisp".into(),
                tone: "calm and helpful".into(),
                language: "English".into(),
                addendum: String::new(),
            })
    }

    /// Tools enabled by the active Loadout (FR-CMD-07). Default Loadout is Full.
    pub fn loadout(&self) -> Vec<crate::tools::ToolName> {
        store::presets::active(&self.db, "loadout")
            .and_then(|p| serde_json::from_value::<store::presets::Loadout>(p.data).ok())
            .map(|l| l.tools)
            .unwrap_or_else(|| crate::tools::ToolName::ALL.to_vec())
    }
}
