//! Wisp: a Windows 11 voice assistant powered by the Gemini API.

pub mod agent;
pub mod apps;
pub mod audio;
pub mod capture;
pub mod clipboard;
pub mod commands;
pub mod cta;
pub mod dictation;
pub mod dispatch;
pub mod error;
pub mod gemini;
pub mod hotkeys;
pub mod input;
pub mod island;
pub mod media;
pub mod notify;
pub mod scheduler;
pub mod secrets;
pub mod state;
pub mod store;
pub mod sysevents;
pub mod taskhub;
pub mod todolist;
pub mod tools;
pub mod voice;
pub mod winutil;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

use crate::gemini::client::{ClientEvent, GeminiClient, DEFAULT_BASE};
use crate::state::{ctx, AppState};
use crate::store::{counters, presets, settings, Db};

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();
static TRAY_PAUSE: Mutex<Option<CheckMenuItem<tauri::Wry>>> = Mutex::new(None);

pub fn data_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("Wisp")
}

pub fn logs_dir() -> Option<PathBuf> {
    std::env::var("LOCALAPPDATA")
        .ok()
        .map(|d| PathBuf::from(d).join("Wisp").join("logs"))
}

/// Daily rolling logs, 7 kept (section 6.1). Events and codes only.
fn init_logging() {
    use tracing_subscriber::prelude::*;
    let filter = tracing_subscriber::EnvFilter::try_from_env("WISP_LOG")
        .unwrap_or_else(|_| "info,wisp_lib=info".into());
    let Some(dir) = logs_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("wisp")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&dir);
    let Ok(appender) = appender else { return };
    let (nb, guard) = tracing_appender::non_blocking(appender);
    let _ = LOG_GUARD.set(guard);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(nb)
        .with_ansi(false)
        .with_target(true);
    let registry = tracing_subscriber::registry().with(filter).with(file_layer);
    if cfg!(debug_assertions) {
        let _ = registry
            .with(tracing_subscriber::fmt::layer().with_target(true))
            .try_init();
    } else {
        let _ = registry.try_init();
    }
}

/// Releases everything Wisp holds (NFR-31).
pub fn shutdown() {
    input::release_all();
    hotkeys::stop();
    tracing::info!("shutdown");
}

pub fn show_main(app: &AppHandle, page: Option<&str>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        if let Some(p) = page {
            let _ = app.emit("wisp://navigate", serde_json::json!({ "page": p }));
        }
    }
}

pub fn update_tray_pause(_app: &AppHandle, paused: bool) {
    if let Some(item) = TRAY_PAUSE.lock().unwrap().as_ref() {
        let _ = item.set_checked(paused);
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Wisp", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause hotkeys", true, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &pause, &sep, &quit])?;
    *TRAY_PAUSE.lock().unwrap() = Some(pause.clone());
    TrayIconBuilder::with_id("wisp")
        .icon(app.default_window_icon().cloned().expect("icon"))
        .tooltip("Wisp")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "open" => show_main(app, None),
            "pause" => {
                let p = !hotkeys::is_paused();
                hotkeys::set_paused(p);
                update_tray_pause(app, p);
                let _ = app.emit(
                    "wisp://settings-changed",
                    serde_json::json!({ "keys": ["hotkeys.paused"] }),
                );
            }
            "quit" => {
                shutdown();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                show_main(tray.app_handle(), None);
            }
        })
        .build(app)?;
    Ok(())
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting");
    let handle = app.handle().clone();

    let report = Db::open(&data_dir().join("wisp.db"))?;
    if let Some(bad) = &report.recovered_from {
        tracing::warn!(path = %bad.display(), "database was corrupted and has been recreated");
        commands::DB_RECOVERED.store(true, std::sync::atomic::Ordering::SeqCst);
        notify::show(
            "Wisp reset its data",
            "The database was damaged, so a fresh one was created. The old file was kept.",
        );
    }
    let db = report.db;
    presets::seed(&db)?;
    let settings = settings::load(&db);

    let counter_db = db.clone();
    let gemini = GeminiClient::new(DEFAULT_BASE).with_observer(Arc::new(move |e| match e {
        ClientEvent::Request => counters::increment(&counter_db, counters::REQUESTS),
        ClientEvent::RateLimited => counters::increment(&counter_db, counters::RATE_LIMITED),
    }));

    let c = state::init(AppState {
        app: handle.clone(),
        db,
        settings: RwLock::new(settings.clone()),
        gemini,
        recorder: audio::Recorder::spawn(),
        apps: apps::AppIndex::default(),
        clip_history: clipboard::History::default(),
        island: island::Island::new(handle.clone()),
        agent: agent::AgentControl::default(),
        scheduler: scheduler::Scheduler::default(),
        cta: cta::CtaState::default(),
        taskhub: taskhub::TaskHub::default(),
        session_locked: AtomicBool::new(false),
        models_cache: Mutex::new(None),
        recording_for: Mutex::new(None),
    });

    island::create(&handle)?;
    cta::create_windows(&handle)?;
    build_tray(&handle)?;

    // Hotkeys: the hook thread feeds a channel; this task routes events.
    let (htx, mut hrx) = tokio::sync::mpsc::unbounded_channel();
    hotkeys::start(
        htx,
        hotkeys::chords_from(
            &settings.hotkeys.dictation,
            &settings.hotkeys.circle,
            &settings.hotkeys.command,
        ),
    );
    tauri::async_runtime::spawn(async move {
        while let Some(e) = hrx.recv().await {
            // In order: handlers do the quick part and spawn the slow part.
            dispatch::on_hook(e).await;
        }
    });

    let (stx, mut srx) = tokio::sync::mpsc::unbounded_channel();
    sysevents::start(stx);
    tauri::async_runtime::spawn(async move {
        while let Some(e) = srx.recv().await {
            dispatch::on_sys(e).await;
        }
    });

    // App index at startup and every 30 min (FR-CMD-05).
    tauri::async_runtime::spawn(async {
        loop {
            let _ = tokio::task::spawn_blocking(|| ctx().apps.refresh()).await;
            tokio::time::sleep(Duration::from_secs(30 * 60)).await;
        }
    });

    c.scheduler.start();

    // TaskHub changes made elsewhere (phone, web) show up within ~2 minutes.
    tauri::async_runtime::spawn(async {
        let mut last: Option<String> = None;
        loop {
            tokio::time::sleep(Duration::from_secs(120)).await;
            if !taskhub::signed_in() {
                continue;
            }
            if let Ok(list) = todolist::refresh().await {
                let sig = serde_json::to_string(&list).unwrap_or_default();
                if last.as_ref() != Some(&sig) {
                    if last.is_some() {
                        ctx().emit("todos-changed", serde_json::json!({}));
                        ctx().scheduler.wake();
                    }
                    last = Some(sig);
                }
            }
        }
    });

    media::watch(|| {
        let info = media::current();
        ctx().emit("media-changed", info);
    });

    // Hide the idle island over full-screen apps (FR-ISL-11).
    std::thread::Builder::new()
        .name("wisp-fullscreen".into())
        .spawn(|| {
            let mut hidden = false;
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let c = ctx();
                let want_hide = c.settings().island.hide_in_fullscreen
                    && c.island.state() == island::State::Idle
                    && winutil::fullscreen_busy();
                if want_hide != hidden {
                    if want_hide {
                        if let Some(w) = c.island.window() {
                            let _ = w.hide();
                        }
                    } else {
                        c.island.show();
                    }
                    hidden = want_hide;
                }
            }
        })?;

    c.island.place(&settings.island.monitor);
    c.island.show();
    island::start_hit_test(handle.clone());

    // Autostart default on (FR-SET-08).
    let al = handle.autolaunch();
    let enabled = al.is_enabled().unwrap_or(false);
    if settings.app.autostart && !enabled {
        let _ = al.enable();
    } else if !settings.app.autostart && enabled {
        let _ = al.disable();
    }

    if !settings.app.first_run_done || c.key().is_none() {
        show_main(&handle, Some("setup"));
    }
    tracing::info!("ready");
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        input::release_all();
        tracing::error!(panic = %info, "panic");
        default_hook(info);
    }));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // FR-SET-08: a second launch focuses the first.
            show_main(app, None);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::secrets_status,
            commands::secrets_set_key,
            commands::secrets_delete_key,
            commands::validate_key,
            commands::list_models,
            commands::settings_get,
            commands::settings_set,
            commands::presets_list,
            commands::presets_save,
            commands::presets_activate,
            commands::presets_delete,
            commands::presets_export,
            commands::presets_import,
            commands::todos_list,
            commands::todos_create,
            commands::todos_update,
            commands::todos_delete,
            commands::todos_reorder,
            commands::todo_snooze,
            commands::taskhub_status,
            commands::taskhub_sign_in,
            commands::taskhub_sign_out,
            commands::jobs_list,
            commands::jobs_create,
            commands::jobs_update,
            commands::jobs_delete,
            commands::jobs_run_now,
            commands::job_runs_list,
            commands::run_text_command,
            commands::run_agent_task,
            commands::agent_stop,
            commands::agent_resume,
            commands::confirm_answer,
            commands::history_commands,
            commands::history_dictations,
            commands::copy_dictation,
            commands::apps_search,
            commands::apps_pinned,
            commands::apps_pin,
            commands::apps_unpin,
            commands::launch_app,
            commands::media_control,
            commands::media_get,
            commands::volume_get,
            commands::volume_set_mute,
            commands::wallpaper_list,
            commands::wallpaper_set,
            commands::wallpaper_pick_folder,
            commands::clipboard_history_list,
            commands::clipboard_history_paste,
            commands::mic_devices_list,
            commands::mic_test,
            commands::monitors_list,
            commands::window_show_main,
            commands::delete_all_data,
            commands::diagnostics_copy,
            commands::hotkeys_set_paused,
            commands::open_external,
            commands::island_layout,
            commands::island_set_interactive,
            commands::overlay_submit,
            commands::card_resize,
            commands::cta_ask,
            commands::cta_close,
            commands::quit_app,
        ])
        .setup(setup)
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Closing the main window hides it; Quit is in the tray (section 3.3).
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Wisp")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                shutdown();
            }
        });
}
