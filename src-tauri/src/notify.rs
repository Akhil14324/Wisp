//! Windows toast notifications, with optional buttons (FR-TODO-03, FR-SCH-07).

use tauri_winrt_notification::{Duration, Toast};

fn app_id() -> &'static str {
    // A release install registers the AUMID on its Start Menu shortcut; dev
    // builds have no shortcut, so borrow PowerShell's id like the Tauri plugin does.
    if cfg!(debug_assertions) {
        Toast::POWERSHELL_APP_ID
    } else {
        "com.personal.wisp"
    }
}

/// Plain notification. Errors are logged, never shown.
pub fn show(title: &str, body: &str) {
    let r = Toast::new(app_id())
        .title(title)
        .text1(body)
        .sound(None)
        .duration(Duration::Short)
        .show();
    if let Err(e) = r {
        tracing::warn!(error = %e, "notification failed");
    }
}

/// Notification with buttons; `on_action` receives the clicked button's action
/// id, or None when the body was clicked.
pub fn show_with_actions(
    title: &str,
    body: &str,
    buttons: &[(&str, &str)],
    on_action: impl FnMut(Option<String>) + Send + 'static,
) {
    let mut cb = on_action;
    let mut t = Toast::new(app_id())
        .title(title)
        .text1(body)
        .sound(None)
        .duration(Duration::Long);
    for (label, action) in buttons {
        t = t.add_button(label, action);
    }
    let t = t.on_activated(move |a| {
        cb(a);
        Ok(())
    });
    if let Err(e) = t.show() {
        tracing::warn!(error = %e, "notification failed");
    }
}
