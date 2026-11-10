//! A silent system notification, for when the window is out of sight (a screenshot was added,
//! the recording went on in the tray).

pub fn show(text: &str) {
    // A driven test run says nothing on the real desktop.
    if crate::drive::on() {
        return;
    }
    #[cfg(windows)]
    {
        use tauri_winrt_notification::Toast;
        // An installed copy has a Start menu shortcut carrying the app's id; a dev build doesn't,
        // and Windows only shows toasts for an id it knows, so it borrows PowerShell's.
        let app_id = if cfg!(debug_assertions) { Toast::POWERSHELL_APP_ID } else { chidori_core::host::IDENTIFIER };
        let result = Toast::new(app_id).title("Chidori").text1(text).sound(None).show();
        if let Err(e) = result {
            tracing::debug!("notification failed: {e}");
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Notification Center through AppleScript: it needs no bundle or permission prompt.
        let quoted = text.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("display notification \"{quoted}\" with title \"Chidori\"");
        if let Err(e) = std::process::Command::new("/usr/bin/osascript").args(["-e", &script]).spawn() {
            tracing::debug!("notification failed: {e}");
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = text;
}
