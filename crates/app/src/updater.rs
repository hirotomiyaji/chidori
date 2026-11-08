//! Daily update check (FR-95, section 17). A found update is a banner whose button installs it;
//! nothing installs without a click, and never while recording.

use std::time::Duration;

use chidori_core::error::{AppError, AppResult, ErrorCode};
use chidori_core::events::{self, Notice, NoticeLevel};
use chidori_core::host::{self, AppHandle};
use chidori_core::recorder::Phase;
use chidori_core::update::{self, Release};
use parking_lot::Mutex;

/// Let the window open first, so launch stays free of network calls while it loads.
const LAUNCH_DELAY: Duration = Duration::from_secs(10);

static FOUND: Mutex<Option<Release>> = Mutex::new(None);

/// Checks at launch when the setting is on and the last check is a day old.
pub fn check_on_launch(app: &AppHandle) {
    let st = app.state();
    if !st.settings.read().updates.check
        || !update::due(st.internal.lock().last_update_check, chrono::Utc::now().timestamp())
    {
        return;
    }
    let app = app.clone();
    host::runtime().spawn(async move {
        tokio::time::sleep(LAUNCH_DELAY).await;
        let st = app.state();
        // Recorded whatever the result, so a failing endpoint isn't asked again every launch.
        st.internal.lock().last_update_check = Some(chrono::Utc::now().timestamp());
        st.persist_internal(&app);
        match update::check(app.version()).await {
            Ok(Some(release)) => {
                tracing::info!("update {} available", release.version);
                let notice = Notice::new(NoticeLevel::Info, "update_available", "updateAvailable")
                    .param("version", release.version.clone())
                    .action("updateNow", "install_update");
                *FOUND.lock() = Some(release);
                events::notice(&app, notice);
            }
            Ok(None) => tracing::info!("no update available"),
            Err(e) => tracing::warn!("update check failed: {e}"),
        }
    });
}

fn while_recording() -> AppError {
    AppError::new(ErrorCode::Internal, "updateWhileRecording")
}

/// Downloads the update the launch check found, checks its signature, and runs the installer in
/// passive mode; the installer closes the app and starts it again (`/R`). Blocking. On macOS it
/// opens the release page instead.
pub fn install(app: &AppHandle) -> AppResult<()> {
    let release = FOUND.lock().clone().ok_or_else(|| AppError::internal("no update found"))?;
    // A Mac update is a .dmg the user drags in: open its page.
    if cfg!(target_os = "macos") {
        return open::that_detached(&release.url)
            .map_err(|e| AppError::internal(format!("opening {}: {e}", release.url)));
    }
    if !matches!(*app.state().recorder.lock(), Phase::Idle) {
        return Err(while_recording());
    }
    tracing::info!("downloading update {}", release.version);
    let bytes = host::runtime().block_on(update::download(&release))?;
    let installer = std::env::temp_dir().join(format!("Chidori_{}_x64-setup.exe", release.version));
    std::fs::write(&installer, &bytes)?;

    // Hold the recorder lock from the last idle check until the installer takes over, so a
    // recording can't start in between.
    let recorder = app.state().recorder.lock();
    if !matches!(*recorder, Phase::Idle) {
        return Err(while_recording());
    }
    std::process::Command::new(&installer)
        .args(["/P", "/UPDATE", "/R"])
        .spawn()
        .map_err(|e| AppError::internal(format!("starting the installer: {e}")))?;
    drop(recorder);
    std::process::exit(0);
}
