/// Commands the front end may call (Appendix B plus the UI helpers listed in
/// docs/DEVIATIONS.md). Each gets an `allow-<name>` permission that the
/// per-window capability files grant (NFR-12).
const COMMANDS: &[&str] = &[
    "app_status",
    "secrets_status",
    "secrets_set_key",
    "secrets_delete_key",
    "validate_key",
    "list_models",
    "settings_get",
    "settings_set",
    "presets_list",
    "presets_save",
    "presets_activate",
    "presets_delete",
    "presets_export",
    "presets_import",
    "todos_list",
    "todos_create",
    "todos_update",
    "todos_delete",
    "todos_reorder",
    "todo_snooze",
    "jobs_list",
    "jobs_create",
    "jobs_update",
    "jobs_delete",
    "jobs_run_now",
    "job_runs_list",
    "run_text_command",
    "run_agent_task",
    "agent_stop",
    "agent_resume",
    "confirm_answer",
    "history_commands",
    "history_dictations",
    "copy_dictation",
    "apps_search",
    "apps_pinned",
    "apps_pin",
    "apps_unpin",
    "launch_app",
    "media_control",
    "media_get",
    "volume_get",
    "volume_set_mute",
    "wallpaper_list",
    "wallpaper_set",
    "wallpaper_pick_folder",
    "clipboard_history_list",
    "clipboard_history_paste",
    "mic_devices_list",
    "mic_test",
    "monitors_list",
    "window_show_main",
    "delete_all_data",
    "diagnostics_copy",
    "hotkeys_set_paused",
    "open_external",
    "island_layout",
    "island_set_interactive",
    "overlay_submit",
    "card_resize",
    "cta_ask",
    "cta_close",
    "quit_app",
];

fn main() {
    let manifest = std::fs::read_to_string("manifest.xml").expect("manifest.xml");
    let windows = tauri_build::WindowsAttributes::new().app_manifest(manifest);
    let attrs = tauri_build::Attributes::new()
        .windows_attributes(windows)
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attrs).expect("tauri build");

    println!("cargo:rerun-if-changed=manifest.xml");
}
