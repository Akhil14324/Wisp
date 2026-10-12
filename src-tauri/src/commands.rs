//! Tauri commands (Appendix B). Each returns `Result<T, UiError>`. Commands
//! marked main-only reject other windows, and no model tool can reach them.

use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::WebviewWindow;
use tauri_plugin_autostart::ManagerExt;

use crate::agent::{self, CommandInput, RunOpts, Source};
use crate::apps::AppEntry;
use crate::capture::{self, MonitorInfo, Stroke};
use crate::error::{ErrCode, UiError, UiResult};
use crate::gemini::client::{GemError, ModelInfo};
use crate::scheduler::{schedule::When, to_utc_text};
use crate::secrets::{self, ApiKey};
use crate::state::ctx;
use crate::store::{self, counters, history, jobs, presets, settings, todos};

fn main_only(w: &WebviewWindow) -> UiResult<()> {
    if w.label() == "main" {
        Ok(())
    } else {
        Err(UiError::of(ErrCode::Forbidden))
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> UiResult<T> {
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(UiError::invalid(e.to_string())),
        Err(_) => Err(UiError::of(ErrCode::Internal)),
    }
}

// ---------- Status ----------

#[derive(Serialize)]
pub struct AppStatus {
    version: &'static str,
    has_key: bool,
    key_masked: Option<String>,
    first_run_done: bool,
    privacy_ack_at: Option<String>,
    hotkeys_paused: bool,
    requests_today: i64,
    rate_limited_today: i64,
    animations: bool,
    live_available: bool,
    db_recovered: bool,
}

pub static DB_RECOVERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
pub fn app_status() -> UiResult<AppStatus> {
    let c = ctx();
    let s = c.settings();
    let key = c.key();
    Ok(AppStatus {
        version: env!("CARGO_PKG_VERSION"),
        has_key: key.is_some(),
        key_masked: key.map(|k| k.masked()),
        first_run_done: s.app.first_run_done,
        privacy_ack_at: s.app.privacy_ack_at,
        hotkeys_paused: crate::hotkeys::is_paused(),
        requests_today: counters::today_value(&c.db, counters::REQUESTS),
        rate_limited_today: counters::today_value(&c.db, counters::RATE_LIMITED),
        animations: crate::winutil::animations_enabled(),
        live_available: crate::gemini::live::AVAILABLE,
        db_recovered: DB_RECOVERED.load(std::sync::atomic::Ordering::SeqCst),
    })
}

// ---------- Key and models ----------

#[derive(Serialize)]
pub struct KeyStatus {
    has_key: bool,
    masked: Option<String>,
}

#[tauri::command]
pub fn secrets_status() -> UiResult<KeyStatus> {
    let k = ctx().key();
    Ok(KeyStatus {
        has_key: k.is_some(),
        masked: k.map(|k| k.masked()),
    })
}

/// Light sanity check only; Google decides whether the key is valid.
/// Drops whitespace and surrounding quotes that often come along with a copy.
fn clean_key(raw: &str) -> UiResult<ApiKey> {
    let k: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let k = k.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    if k.len() < 10 || k.len() > 512 || !k.chars().all(|c| c.is_ascii_graphic()) {
        return Err(UiError::invalid(
            "That does not look like an API key. Copy it again from Google AI Studio.",
        ));
    }
    Ok(ApiKey::new(k))
}

#[cfg(test)]
mod tests {
    use super::clean_key;

    #[test]
    fn key_cleaning() {
        assert_eq!(
            clean_key("  AIzaSyABC123-_xyz789qwe \n").unwrap().expose(),
            "AIzaSyABC123-_xyz789qwe"
        );
        assert_eq!(
            clean_key("\"AQ.Ab8RN6abc.def-123\"").unwrap().expose(),
            "AQ.Ab8RN6abc.def-123"
        );
        assert_eq!(
            clean_key("AIzaSy\r\nABC123xyz789").unwrap().expose(),
            "AIzaSyABC123xyz789"
        );
        assert!(clean_key("short").is_err());
        assert!(clean_key("ключключключключ").is_err());
    }
}

#[tauri::command]
pub async fn secrets_set_key(window: WebviewWindow, key: String) -> UiResult<KeyStatus> {
    main_only(&window)?;
    let k = clean_key(&key)?;
    secrets::set(&k).map_err(|e| UiError::internal(e.to_string()))?;
    *ctx().models_cache.lock().unwrap() = None;
    tracing::info!("api key stored");
    secrets_status()
}

#[tauri::command]
pub fn secrets_delete_key(window: WebviewWindow) -> UiResult<KeyStatus> {
    main_only(&window)?;
    secrets::delete().map_err(|e| UiError::internal(e.to_string()))?;
    tracing::info!("api key deleted");
    secrets_status()
}

#[derive(Serialize)]
pub struct KeyCheck {
    ok: bool,
    model_count: usize,
    status: Option<u16>,
    cause: Option<String>,
}

fn plain_cause(e: &GemError) -> String {
    match e {
        GemError::KeyInvalid(_) => {
            "Gemini rejected the key. Check that you copied all of it.".into()
        }
        GemError::RateLimit => {
            "The key works but its quota is used up right now. Try again later.".into()
        }
        GemError::Network | GemError::Timeout => "No network connection to Gemini.".into(),
        GemError::Http(_, m) if !m.is_empty() => format!("Gemini said: {m}"),
        other => format!("Gemini returned an error ({})", other.code().as_str()),
    }
}

/// FR-SET-04: validates a key (the given one, or the stored one) via the models list.
#[tauri::command]
pub async fn validate_key(window: WebviewWindow, key: Option<String>) -> UiResult<KeyCheck> {
    main_only(&window)?;
    let k = match key {
        Some(raw) => clean_key(&raw)?,
        None => ctx().key().ok_or_else(|| UiError::of(ErrCode::NoKey))?,
    };
    match ctx().gemini.list_models(&k).await {
        Ok(models) => {
            let count = models.len();
            *ctx().models_cache.lock().unwrap() = Some(models);
            Ok(KeyCheck {
                ok: true,
                model_count: count,
                status: Some(200),
                cause: None,
            })
        }
        Err(e) => Ok(KeyCheck {
            ok: false,
            model_count: 0,
            status: e.status(),
            cause: Some(plain_cause(&e)),
        }),
    }
}

#[derive(Serialize)]
pub struct ModelsList {
    models: Vec<ModelInfo>,
    /// Role -> configured default missing from the list (FR-SET-07 warning).
    missing: Vec<String>,
}

#[tauri::command]
pub async fn list_models() -> UiResult<ModelsList> {
    let c = ctx();
    let cached = c.models_cache.lock().unwrap().clone();
    let models = match cached {
        Some(m) => m,
        None => {
            let key = c.key().ok_or_else(|| UiError::of(ErrCode::NoKey))?;
            let m = c.gemini.list_models(&key).await?;
            *c.models_cache.lock().unwrap() = Some(m.clone());
            m
        }
    };
    let s = c.settings();
    let mut missing = vec![];
    for (role, id) in [
        ("dictation", &s.models.dictation),
        ("command", &s.models.command),
        ("vision", &s.models.vision),
        ("computer_use", &s.models.computer_use),
    ] {
        if !models.iter().any(|m| &m.id == id) {
            missing.push(role.to_string());
        }
    }
    Ok(ModelsList { models, missing })
}

// ---------- Settings ----------

#[tauri::command]
pub fn settings_get() -> UiResult<Value> {
    Ok(store::settings::load_tree(&ctx().db))
}

#[tauri::command]
pub fn settings_set(window: WebviewWindow, key: String, value: Value) -> UiResult<Value> {
    if settings::MAIN_ONLY_KEYS.contains(&key.as_str()) {
        main_only(&window)?;
    }
    apply_setting(&key, value)?;
    Ok(store::settings::load_tree(&ctx().db))
}

/// Stores a setting and applies its side effects live.
pub fn apply_setting(key: &str, value: Value) -> UiResult<()> {
    let c = ctx();
    let s = settings::set(&c.db, key, value)?;
    c.reload_settings();
    match key {
        k if k.starts_with("hotkeys.") => crate::hotkeys::reload(crate::hotkeys::chords_from(
            &s.hotkeys.dictation,
            &s.hotkeys.circle,
            &s.hotkeys.command,
        )),
        "app.autostart" => {
            let al = c.app.autolaunch();
            let r = if s.app.autostart {
                al.enable()
            } else {
                al.disable()
            };
            if let Err(e) = r {
                tracing::warn!(error = %e, "autostart change failed");
            }
        }
        "dictation.history_enabled" if !s.dictation.history_enabled => {
            history::clear_dictations(&c.db)?;
        }
        "clipboard.history_enabled" if !s.clipboard.history_enabled => c.clip_history.clear(),
        "island.monitor" => c.island.place(&s.island.monitor),
        _ => {}
    }
    c.emit("settings-changed", json!({ "keys": [key] }));
    Ok(())
}

// ---------- Presets ----------

#[tauri::command]
pub fn presets_list() -> UiResult<Vec<presets::Preset>> {
    Ok(presets::list(&ctx().db)?)
}

#[tauri::command]
pub fn presets_save(
    id: Option<i64>,
    slot: String,
    name: String,
    data: Value,
) -> UiResult<presets::Preset> {
    let p = presets::save(&ctx().db, id, &slot, &name, &data)?;
    if p.is_active {
        apply_preset(&p)?;
    }
    ctx().emit("presets-changed", json!({ "slot": slot }));
    Ok(p)
}

fn apply_preset(p: &presets::Preset) -> UiResult<()> {
    if p.slot == "keys" {
        let k: presets::KeysPreset = serde_json::from_value(p.data.clone())
            .map_err(|_| UiError::invalid("bad keys preset"))?;
        apply_setting("hotkeys.dictation", json!(k.dictation))?;
        apply_setting("hotkeys.circle", json!(k.circle))?;
        apply_setting("hotkeys.command", json!(k.command))?;
        apply_setting("search.default_engine", json!(k.search_engine))?;
        apply_setting("audio.input_device", json!(k.input_device))?;
        apply_setting("capture.target_monitor", json!(k.target_monitor))?;
    }
    Ok(())
}

#[tauri::command]
pub fn presets_activate(id: i64) -> UiResult<presets::Preset> {
    let p = presets::activate(&ctx().db, id)?;
    apply_preset(&p)?;
    ctx().emit("presets-changed", json!({ "slot": p.slot }));
    Ok(p)
}

#[tauri::command]
pub fn presets_delete(id: i64) -> UiResult<()> {
    presets::delete(&ctx().db, id)?;
    ctx().emit("presets-changed", json!({}));
    Ok(())
}

#[tauri::command]
pub fn presets_export(id: i64) -> UiResult<String> {
    let p = presets::get(&ctx().db, id)?.ok_or_else(|| UiError::invalid("No such preset"))?;
    serde_json::to_string_pretty(&presets::export_doc(&p))
        .map_err(|e| UiError::internal(e.to_string()))
}

#[tauri::command]
pub fn presets_import(text: String) -> UiResult<presets::Preset> {
    if text.len() > 64 * 1024 {
        return Err(UiError::invalid("That file is too large to be a preset"));
    }
    let (slot, name, data) = presets::parse_import(&text)?;
    let db = &ctx().db;
    let mut final_name = name.trim().chars().take(30).collect::<String>();
    if final_name.is_empty() {
        final_name = "Imported".into();
    }
    let existing: Vec<String> = presets::list(db)?
        .into_iter()
        .filter(|p| p.slot == slot)
        .map(|p| p.name)
        .collect();
    let mut candidate = final_name.clone();
    let mut n = 2;
    while existing.contains(&candidate) {
        candidate = format!("{final_name} ({n})");
        n += 1;
    }
    let p = presets::save(db, None, &slot, &candidate, &data)?;
    ctx().emit("presets-changed", json!({ "slot": slot }));
    Ok(p)
}

// ---------- To-do ----------

#[tauri::command]
pub fn todos_list() -> UiResult<Vec<todos::Todo>> {
    Ok(todos::list(&ctx().db)?)
}

fn validate_todo_title(t: &str) -> UiResult<()> {
    if t.trim().is_empty() || t.len() > 200 {
        return Err(UiError::invalid("Titles are 1 to 200 characters"));
    }
    Ok(())
}

fn validate_iso(s: &Option<String>) -> UiResult<()> {
    if let Some(v) = s {
        chrono::DateTime::parse_from_rfc3339(v)
            .map_err(|_| UiError::invalid("Due time is not a valid date"))?;
    }
    Ok(())
}

fn todos_changed() {
    let c = ctx();
    c.emit("todos-changed", json!({}));
    c.scheduler.wake();
}

#[tauri::command]
pub fn todos_create(todo: todos::NewTodo) -> UiResult<todos::Todo> {
    validate_todo_title(&todo.title)?;
    validate_iso(&todo.due_at)?;
    let t = todos::create(&ctx().db, &todo)?;
    todos_changed();
    Ok(t)
}

#[tauri::command]
pub fn todos_update(patch: todos::TodoPatch) -> UiResult<Option<todos::Todo>> {
    if let Some(t) = &patch.title {
        validate_todo_title(t)?;
    }
    if let Some(d) = &patch.due_at {
        validate_iso(d)?;
    }
    let t = todos::update(&ctx().db, &patch)?;
    todos_changed();
    Ok(t)
}

#[tauri::command]
pub fn todos_delete(id: i64) -> UiResult<()> {
    todos::delete(&ctx().db, id)?;
    todos_changed();
    Ok(())
}

#[tauri::command]
pub fn todos_reorder(ids: Vec<i64>) -> UiResult<()> {
    todos::reorder(&ctx().db, &ids)?;
    todos_changed();
    Ok(())
}

#[tauri::command]
pub fn todo_snooze(id: i64, minutes: Option<i64>) -> UiResult<()> {
    let m = minutes.unwrap_or(10).clamp(1, 24 * 60);
    let until = to_utc_text(chrono::Local::now() + chrono::Duration::minutes(m));
    todos::snooze(&ctx().db, id, &until)?;
    todos_changed();
    Ok(())
}

// ---------- Jobs ----------

#[tauri::command]
pub fn jobs_list() -> UiResult<Vec<jobs::Job>> {
    Ok(jobs::list(&ctx().db)?)
}

#[derive(Deserialize)]
pub struct JobInput {
    name: String,
    command: String,
    when: When,
    #[serde(default)]
    allow_computer_use: bool,
    #[serde(default = "yes")]
    enabled: bool,
}

fn yes() -> bool {
    true
}

fn validate_job(j: &JobInput) -> UiResult<()> {
    if j.name.trim().is_empty() || j.name.len() > 80 {
        return Err(UiError::invalid("Names are 1 to 80 characters"));
    }
    if j.command.trim().is_empty() || j.command.len() > 500 {
        return Err(UiError::invalid("Commands are 1 to 500 characters"));
    }
    j.when
        .validate(chrono::Local::now())
        .map_err(UiError::invalid)
}

fn jobs_changed() {
    let c = ctx();
    c.emit("jobs-changed", json!({}));
    c.scheduler.wake();
}

#[tauri::command]
pub fn jobs_create(window: WebviewWindow, job: JobInput) -> UiResult<jobs::Job> {
    if job.allow_computer_use {
        main_only(&window)?;
    }
    validate_job(&job)?;
    let next = job.when.next_after(chrono::Local::now()).map(to_utc_text);
    let j = jobs::create(
        &ctx().db,
        job.name.trim(),
        job.command.trim(),
        &job.when,
        job.allow_computer_use,
        next,
    )?;
    jobs_changed();
    Ok(j)
}

#[tauri::command]
pub fn jobs_update(window: WebviewWindow, id: i64, job: JobInput) -> UiResult<jobs::Job> {
    let db = &ctx().db;
    let mut existing = jobs::get(db, id)?.ok_or_else(|| UiError::invalid("No such job"))?;
    if job.allow_computer_use != existing.allow_computer_use {
        // NFR-16: only the main window may change Allow Computer Use.
        main_only(&window)?;
    }
    let when_changed = job.when != existing.when;
    if when_changed || job.enabled != existing.enabled {
        if when_changed {
            validate_job(&job)?;
        }
    } else if job.name.trim().is_empty() || job.command.trim().is_empty() {
        return Err(UiError::invalid("Name and command are required"));
    }
    existing.name = job.name.trim().to_string();
    existing.command = job.command.trim().to_string();
    existing.allow_computer_use = job.allow_computer_use;
    existing.enabled = job.enabled;
    if when_changed || existing.next_run_at.is_none() {
        existing.next_run_at = job.when.next_after(chrono::Local::now()).map(to_utc_text);
    }
    existing.when = job.when;
    jobs::save(db, &existing)?;
    jobs_changed();
    Ok(existing)
}

#[tauri::command]
pub fn jobs_delete(id: i64) -> UiResult<()> {
    jobs::delete(&ctx().db, id)?;
    jobs_changed();
    Ok(())
}

#[tauri::command]
pub fn jobs_run_now(id: i64) -> UiResult<()> {
    let c = ctx();
    let job = jobs::get(&c.db, id)?.ok_or_else(|| UiError::invalid("No such job"))?;
    c.scheduler
        .enqueue(job, to_utc_text(chrono::Local::now()))
        .map_err(UiError::invalid)
}

#[tauri::command]
pub fn job_runs_list(job_id: Option<i64>) -> UiResult<Vec<jobs::JobRun>> {
    Ok(jobs::runs(&ctx().db, job_id)?)
}

// ---------- Agent ----------

#[derive(Serialize)]
pub struct CommandResult {
    text: String,
    ok: bool,
}

#[tauri::command]
pub async fn run_text_command(text: String) -> UiResult<CommandResult> {
    let t: String = text.trim().chars().take(2000).collect();
    if t.is_empty() {
        return Err(UiError::invalid("Type a command first"));
    }
    let r = agent::run_command(
        CommandInput::Text(t),
        RunOpts {
            source: Source::Typed,
            allowed: ctx().loadout(),
            unattended: false,
        },
    )
    .await?;
    Ok(CommandResult {
        text: r.text,
        ok: r.ok,
    })
}

#[tauri::command]
pub async fn run_agent_task(window: WebviewWindow, goal: String) -> UiResult<CommandResult> {
    main_only(&window)?;
    let c = ctx();
    if !c
        .loadout()
        .contains(&crate::tools::ToolName::RunComputerTask)
    {
        return Err(UiError::invalid(
            "Computer Use is not enabled in the active Loadout",
        ));
    }
    let run = c
        .agent
        .begin()
        .ok_or_else(|| UiError::invalid("Wisp is busy with another task"))?;
    let r = agent::computer::run_task(&goal, &run.token, false).await;
    c.agent.end(&run);
    match r {
        Ok(s) => {
            c.island.result(&s);
            Ok(CommandResult { text: s, ok: true })
        }
        Err(e) => {
            if e == "Stopped" {
                c.island.idle();
            } else {
                c.island.error_text(&e);
            }
            Ok(CommandResult { text: e, ok: false })
        }
    }
}

#[tauri::command]
pub fn agent_stop() -> UiResult<()> {
    ctx().agent.stop();
    ctx().island.idle();
    Ok(())
}

#[tauri::command]
pub fn agent_resume() -> UiResult<()> {
    let c = ctx();
    c.agent.resume();
    c.emit("agent-paused", json!({ "paused": false }));
    c.island.set(crate::island::State::Acting, Some("Resuming"));
    Ok(())
}

#[tauri::command]
pub fn confirm_answer(id: u64, yes: bool) -> UiResult<bool> {
    let ok = ctx().agent.answer(id, yes);
    if !ok {
        return Err(UiError::invalid("That question already expired"));
    }
    ctx()
        .island
        .set(crate::island::State::Thinking, Some("Thinking"));
    Ok(true)
}

// ---------- History ----------

#[tauri::command]
pub fn history_commands() -> UiResult<Vec<history::CommandEntry>> {
    Ok(history::commands(&ctx().db, 100)?)
}

#[tauri::command]
pub fn history_dictations() -> UiResult<Vec<history::DictationEntry>> {
    Ok(history::dictations(&ctx().db)?)
}

#[tauri::command]
pub async fn copy_dictation(id: i64) -> UiResult<()> {
    let text = history::dictation_text(&ctx().db, id)?;
    blocking(move || crate::clipboard::write_text(&text)).await
}

// ---------- Apps and media ----------

#[tauri::command]
pub fn apps_search(query: String) -> UiResult<Vec<AppEntry>> {
    Ok(ctx()
        .apps
        .search(&query.chars().take(100).collect::<String>(), 30))
}

#[derive(Serialize)]
pub struct Pinned {
    app_id: String,
    label: String,
    position: i64,
}

#[tauri::command]
pub fn apps_pinned() -> UiResult<Vec<Pinned>> {
    Ok(ctx().db.with(|c| {
        let mut st =
            c.prepare("SELECT app_id, label, position FROM pinned_apps ORDER BY position")?;
        let rows = st.query_map([], |r| {
            Ok(Pinned {
                app_id: r.get(0)?,
                label: r.get(1)?,
                position: r.get(2)?,
            })
        })?;
        rows.collect()
    })?)
}

#[tauri::command]
pub fn apps_pin(app_id: String) -> UiResult<()> {
    let c = ctx();
    let entry = c
        .apps
        .find(&app_id)
        .ok_or_else(|| UiError::invalid("Unknown app"))?;
    let count: i64 =
        c.db.with(|db| db.query_row("SELECT COUNT(*) FROM pinned_apps", [], |r| r.get(0)))?;
    if count >= 12 {
        return Err(UiError::invalid("Up to 12 apps can be pinned"));
    }
    c.db.with(|db| {
        db.execute(
            "INSERT OR IGNORE INTO pinned_apps (app_id, label, position) VALUES (?1, ?2, ?3)",
            rusqlite::params![entry.id, entry.name, count],
        )
    })?;
    c.emit("pins-changed", json!({}));
    Ok(())
}

#[tauri::command]
pub fn apps_unpin(app_id: String) -> UiResult<()> {
    ctx()
        .db
        .with(|db| db.execute("DELETE FROM pinned_apps WHERE app_id = ?1", [app_id]))?;
    ctx().emit("pins-changed", json!({}));
    Ok(())
}

#[tauri::command]
pub async fn launch_app(app_id: String) -> UiResult<()> {
    let entry = ctx()
        .apps
        .find(&app_id)
        .ok_or_else(|| UiError::invalid("Unknown app"))?;
    blocking(move || crate::apps::launch(&entry)).await
}

#[tauri::command]
pub async fn media_control(action: String) -> UiResult<()> {
    let a = crate::media::MediaAction::parse(&action)
        .ok_or_else(|| UiError::invalid("Unknown media action"))?;
    blocking(move || crate::media::control(a)).await
}

#[tauri::command]
pub async fn media_get() -> UiResult<crate::media::MediaInfo> {
    blocking(|| Ok(crate::media::current())).await
}

#[derive(Serialize)]
pub struct VolumeInfo {
    level: u32,
    muted: bool,
}

#[tauri::command]
pub async fn volume_get() -> UiResult<VolumeInfo> {
    let (level, muted) = blocking(crate::media::get_volume).await?;
    Ok(VolumeInfo { level, muted })
}

#[tauri::command]
pub async fn volume_set_mute(muted: bool) -> UiResult<()> {
    blocking(move || crate::media::set_volume(None, Some(muted))).await
}

#[derive(Serialize, Clone)]
pub struct Wallpaper {
    path: String,
    name: String,
    thumb: String,
}

fn wallpaper_folder() -> UiResult<std::path::PathBuf> {
    let f = ctx()
        .settings()
        .wallpaper
        .folder
        .ok_or_else(|| UiError::invalid("Pick a wallpaper folder in Settings first"))?;
    let p = std::path::PathBuf::from(f);
    if !p.is_dir() {
        return Err(UiError::invalid("The wallpaper folder no longer exists"));
    }
    Ok(p)
}

#[tauri::command]
pub async fn wallpaper_list() -> UiResult<Vec<Wallpaper>> {
    let dir = wallpaper_folder()?;
    blocking(move || {
        let mut out = vec![];
        for e in std::fs::read_dir(&dir)?.flatten().take(200) {
            let p = e.path();
            let ext = p
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if !crate::media::IMAGE_EXTS.contains(&ext.as_str()) {
                continue;
            }
            let Ok(img) = image::open(&p) else { continue };
            let t = img.thumbnail(160, 100).to_rgba8();
            let Ok(jpg) = capture::to_jpeg(&t, 75) else {
                continue;
            };
            out.push(Wallpaper {
                path: p.to_string_lossy().to_string(),
                name: p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                thumb: format!(
                    "data:image/jpeg;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(jpg)
                ),
            });
            if out.len() >= 60 {
                break;
            }
        }
        Ok(out)
    })
    .await
}

#[tauri::command]
pub async fn wallpaper_set(path: String) -> UiResult<()> {
    let dir = wallpaper_folder()?;
    let p = std::fs::canonicalize(&path)
        .map_err(|_| UiError::invalid("That image no longer exists"))?;
    let d =
        std::fs::canonicalize(&dir).map_err(|_| UiError::invalid("Wallpaper folder missing"))?;
    if !p.starts_with(&d) {
        return Err(UiError::invalid(
            "Only images from the wallpaper folder can be used here",
        ));
    }
    blocking(move || crate::media::set_wallpaper(&p.to_string_lossy())).await
}

#[tauri::command]
pub fn clipboard_history_list() -> UiResult<Vec<crate::clipboard::ClipItem>> {
    Ok(ctx().clip_history.list())
}

#[tauri::command]
pub async fn clipboard_history_paste(id: u64) -> UiResult<()> {
    let text = ctx()
        .clip_history
        .get(id)
        .ok_or_else(|| UiError::invalid("That item is gone"))?;
    blocking(move || crate::dictation::insert_text(&text).map(|_| ())).await
}

// ---------- System ----------

#[tauri::command]
pub async fn mic_devices_list() -> UiResult<Vec<String>> {
    blocking(|| Ok(crate::audio::input_devices())).await
}

#[derive(Serialize)]
pub struct MicTest {
    ok: bool,
    peak: f32,
    message: String,
}

/// FR-SET-06: records 3 s and plays it back.
#[tauri::command]
pub async fn mic_test() -> UiResult<MicTest> {
    let c = ctx();
    if c.recorder.is_recording() {
        return Err(UiError::invalid("The microphone is busy"));
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    c.recorder
        .start(c.settings().audio.input_device, tx)
        .await
        .map_err(|e| UiError::invalid(e.to_string()))?;
    let fwd = tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if let crate::audio::AudioEvent::Level(rms) = ev {
                ctx().emit("audio-level", json!({ "rms": rms }));
            }
        }
    });
    tokio::time::sleep(Duration::from_secs(3)).await;
    let rec = c
        .recorder
        .stop()
        .await
        .map_err(|e| UiError::invalid(e.to_string()));
    fwd.abort();
    let rec = rec?;
    let peak = rec.native.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak < 0.002 {
        return Ok(MicTest {
            ok: false,
            peak,
            message:
                "No sound was picked up. Windows may be blocking the microphone, or it is muted."
                    .into(),
        });
    }
    let (native, rate) = (rec.native, rec.native_rate);
    blocking(move || crate::audio::play(&native, rate)).await?;
    Ok(MicTest {
        ok: true,
        peak,
        message: "Recorded 3 seconds and played it back.".into(),
    })
}

#[tauri::command]
pub async fn monitors_list() -> UiResult<Vec<MonitorInfo>> {
    blocking(|| Ok(capture::monitors())).await
}

#[tauri::command]
pub fn window_show_main(page: Option<String>) -> UiResult<()> {
    crate::show_main(&ctx().app, page.as_deref());
    Ok(())
}

/// Delete all data (section 8.2, NFR-22): wipes the database and logs.
#[tauri::command]
pub fn delete_all_data(window: WebviewWindow, remove_key: bool) -> UiResult<()> {
    main_only(&window)?;
    let c = ctx();
    c.db.wipe_all()?;
    presets::seed(&c.db)?;
    c.reload_settings();
    c.clip_history.clear();
    if let Some(dir) = crate::logs_dir() {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    if remove_key {
        let _ = secrets::delete();
    }
    let s = c.settings();
    crate::hotkeys::reload(crate::hotkeys::chords_from(
        &s.hotkeys.dictation,
        &s.hotkeys.circle,
        &s.hotkeys.command,
    ));
    for t in [
        "settings-changed",
        "todos-changed",
        "jobs-changed",
        "history-changed",
        "presets-changed",
        "pins-changed",
    ] {
        c.emit(t, json!({}));
    }
    tracing::info!(remove_key, "all data deleted");
    Ok(())
}

/// Settings > About: version, Windows build, model IDs, counters. No content or key.
#[tauri::command]
pub async fn diagnostics_copy() -> UiResult<String> {
    let c = ctx();
    let s = c.settings();
    let text = format!(
        "Wisp {}\n{}\nModels: dictation={} command={} vision={} computer_use={}\nRequests today: {}\nHTTP 429 today: {}\nKey stored: {}\nHotkeys paused: {}\nMonitors: {}",
        env!("CARGO_PKG_VERSION"),
        blocking(|| Ok(crate::winutil::windows_build())).await.unwrap_or_default(),
        s.models.dictation,
        s.models.command,
        s.models.vision,
        s.models.computer_use,
        counters::today_value(&c.db, counters::REQUESTS),
        counters::today_value(&c.db, counters::RATE_LIMITED),
        c.key().is_some(),
        crate::hotkeys::is_paused(),
        capture::monitors().len(),
    );
    let t2 = text.clone();
    blocking(move || crate::clipboard::write_text(&t2)).await?;
    Ok(text)
}

#[tauri::command]
pub fn hotkeys_set_paused(paused: bool) -> UiResult<()> {
    crate::hotkeys::set_paused(paused);
    crate::update_tray_pause(&ctx().app, paused);
    ctx().emit("settings-changed", json!({ "keys": ["hotkeys.paused"] }));
    Ok(())
}

/// Opens one of a fixed set of external targets (wizard, Settings).
#[tauri::command]
pub fn open_external(target: String) -> UiResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let app = &ctx().app;
    let r = match target.as_str() {
        "aistudio" => app
            .opener()
            .open_url("https://aistudio.google.com/apikey", None::<&str>),
        "mic_privacy" => app
            .opener()
            .open_url("ms-settings:privacy-microphone", None::<&str>),
        "typing_settings" => app.opener().open_url("ms-settings:typing", None::<&str>),
        "logs" => match crate::logs_dir() {
            Some(d) => app
                .opener()
                .open_path(d.to_string_lossy().to_string(), None::<&str>),
            None => return Err(UiError::invalid("No log folder")),
        },
        _ => return Err(UiError::invalid("Unknown target")),
    };
    r.map_err(|e| UiError::internal(e.to_string()))
}

// ---------- Island, overlay, card ----------

#[tauri::command]
pub fn island_layout(width: f64, height: f64) -> UiResult<()> {
    let c = ctx();
    c.island.layout(width, height, &c.settings().island.monitor);
    Ok(())
}

#[tauri::command]
pub fn island_set_interactive(on: bool) -> UiResult<()> {
    crate::island::set_interactive(&ctx().app, on);
    Ok(())
}

#[tauri::command]
pub fn overlay_submit(strokes: Vec<Stroke>) -> UiResult<()> {
    let strokes = strokes
        .into_iter()
        .take(50)
        .map(|s| Stroke {
            points: s.points.into_iter().take(5000).collect(),
        })
        .collect();
    crate::cta::submit_strokes(strokes);
    Ok(())
}

#[tauri::command]
pub fn card_resize(height: f64) -> UiResult<()> {
    crate::cta::resize_card(height);
    Ok(())
}

#[tauri::command]
pub async fn cta_ask(text: String) -> UiResult<String> {
    crate::cta::ask_text(&text).await
}

#[tauri::command]
pub fn cta_close() -> UiResult<()> {
    crate::cta::close_card();
    Ok(())
}

#[tauri::command]
pub fn wallpaper_pick_folder(window: WebviewWindow, folder: String) -> UiResult<()> {
    main_only(&window)?;
    let p = std::path::Path::new(folder.trim());
    if !p.is_dir() {
        return Err(UiError::invalid("That folder does not exist"));
    }
    apply_setting("wallpaper.folder", json!(p.to_string_lossy()))
}

#[tauri::command]
pub fn quit_app() {
    crate::shutdown();
    ctx().app.exit(0);
}
