//! The real `App` on a core whose folders are in a temporary directory, for tests that drive its
//! state. (The Iced build also drew screens to PNG here; GPUI has no headless renderer yet, so a
//! screen is checked by running the app.)

use std::sync::Arc;

use chidori_core::asr::worker::AsrWorker;
use chidori_core::host::{AppHandle, Paths};
use chidori_core::state::{AppHost, AppState};

use crate::app::App;

/// A core on a temporary folder: no models, no sessions, default settings.
pub fn core() -> (AppHandle, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let base = dir.path().to_path_buf();
    let paths = Paths {
        config: base.join("config"),
        local_data: base.join("local"),
        logs: base.join("logs"),
        resources: base.join("resources"),
        documents: base.join("documents"),
    };
    crate::bridge::init();
    let core = AppHandle::new(paths, "0.0.0", Arc::new(crate::bridge::Events), Arc::new(crate::bridge::AppShell));
    let worker = AsrWorker::spawn(Arc::new(AppHost { app: core.clone() }));
    core.manage(AppState::new(&core, worker));
    (core, dir)
}

/// The app as it starts, with setup done so the main view shows.
pub fn app() -> (App, tempfile::TempDir) {
    let (core, dir) = core();
    let (mut app, _) = App::new(core);
    // A fresh settings file takes the language of Windows, so pin it or an English runner sees English.
    app.settings.locale = chidori_core::settings::Locale::Ja;
    app.info.setup_done = true;
    app.rec.state = chidori_core::events::UiState::Ready;
    (app, dir)
}
