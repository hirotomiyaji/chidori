//! Chidori: live transcription with local Whisper. The window is GPUI; everything it shows comes
//! from `chidori-core`.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod bridge;
#[cfg(feature = "drive")]
mod drive;
/// Without the `drive` feature nothing drives the app (`drive.rs`).
#[cfg(not(feature = "drive"))]
mod drive {
    pub fn on() -> bool {
        false
    }

    pub fn visible() -> bool {
        true
    }

    pub fn paths() -> Option<chidori_core::host::Paths> {
        None
    }

    pub fn seed(_: &chidori_core::host::AppHandle) {}

    pub fn popup(_: &[crate::native::menu::Item]) -> bool {
        false
    }
}
mod i18n;
mod icons;
mod jello;
mod model;
mod motion;
mod native;
mod paint;
#[cfg(test)]
mod preview;
mod screens;
mod scrollbar;
mod task;
mod theme;
mod ui;
mod updater;
mod widgets;

use std::sync::Arc;

use chidori_core::asr::worker::AsrWorker;
use chidori_core::events::{self, Notice, NoticeLevel};
use chidori_core::host::{AppHandle, Paths};
use chidori_core::state::{self, AppHost, AppState};
use chidori_core::{asr, migrate, net, platform, recorder};

fn main() {
    // Before anything touches Vulkan (section 8).
    asr::engine::disable_vulkan_implicit_layers();
    net::ensure_crypto_provider();
    bridge::init();
    // A driven test run keeps to its own folders and runs beside the app itself.
    let driven = drive::paths();
    // A second launch shows the running window and leaves (FR-91).
    if driven.is_none() && !native::window::single_instance() {
        return;
    }
    // Before anything opens a file in the app's folders.
    let migrated = if driven.is_none() { migrate::app_folders() } else { Vec::new() };

    let mut paths = driven.unwrap_or_else(Paths::system);
    // An installed copy has its resources beside the exe; a dev build reads them from the crate.
    if cfg!(debug_assertions) && !paths.resources.join(chidori_core::models::benchmark::CLIP).exists() {
        paths.resources = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    }
    let _log = init_logging(&paths);
    asr::engine::install_logging_hooks();
    tracing::info!(
        "Chidori {} starting ({} {}, GPU backend compiled: {})",
        env!("CARGO_PKG_VERSION"),
        if cfg!(target_os = "macos") { "macOS" } else { "Windows build" },
        platform::os_build(),
        asr::engine::gpu_compiled()
    );
    for line in &migrated {
        tracing::info!("{line}");
    }

    let core = AppHandle::new(paths, env!("CARGO_PKG_VERSION"), Arc::new(bridge::Events), Arc::new(bridge::AppShell));
    log_panics(core.clone());
    let worker = AsrWorker::spawn(Arc::new(AppHost { app: core.clone() }));
    let _stop_worker = StopWorker(worker.clone());
    core.manage(AppState::new(&core, worker));
    bridge::set_core(core.clone());
    drive::seed(&core);
    let st = core.state();
    state::check_gpu_crash_marker(&core, st);
    let locale = st.settings.read().locale;
    i18n::set_locale(locale);

    let native = {
        let core = core.clone();
        move || {
            // A driven run has no tray and takes no hotkeys from the app itself.
            if drive::on() {
                return;
            }
            native::menu::init();
            native::tray::create(locale);
            native::hotkeys::init();
            native::hotkeys::register_all(&core);
        }
    };
    // On macOS the first NSApplication call fixes its class, and GPUI's must be its own: the tray,
    // menus and hotkeys are made once GPUI runs.
    #[cfg(not(target_os = "macos"))]
    native();
    recorder::spawn_ticker(core.clone());

    if cpu_supported() {
        st.load_selected_model();
    } else {
        tracing::error!("CPU lacks AVX2; transcription is unavailable");
        events::notice(&core, Notice::new(NoticeLevel::Error, "E_INTERNAL", "cpuUnsupported"));
    }
    if !drive::on() {
        updater::check_on_launch(&core);
    }

    #[cfg(target_os = "macos")]
    app::run(native);
    #[cfg(not(target_os = "macos"))]
    app::run(|| {});
}

/// Stops the transcription worker when `main` ends, a panic included, so the Whisper model is
/// freed before the process exits. On Metal, ggml aborts at exit if a model is still loaded.
/// macOS exits without returning from `app::run`, so there the quit hook in `app::run` stops it
/// first; stopping twice is harmless.
struct StopWorker(Arc<AsrWorker>);

impl Drop for StopWorker {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

/// Logs to `logs\chidori.log` (a new file each day, five kept) and, in dev builds, the console.
fn init_logging(paths: &Paths) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt::time::LocalTime;
    use tracing_subscriber::prelude::*;

    // whisper.cpp and the windowing crates are chatty at info level. `CHIDORI_LOG` overrides it
    // (`CHIDORI_LOG=debug`) for a closer look.
    let filter = || {
        EnvFilter::try_from_env("CHIDORI_LOG").unwrap_or_else(|_| {
            EnvFilter::new("info,whisper_rs=warn,wasapi=warn,reqwest=warn,gpui=warn,gpui_pre=warn,gpui_component=warn")
        })
    };
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("chidori")
        .filename_suffix("log")
        .max_log_files(5)
        .build(&paths.logs);
    let (writer, guard) = match file {
        Ok(f) => {
            let (w, g) = tracing_appender::non_blocking(f);
            (Some(w), Some(g))
        }
        Err(e) => {
            eprintln!("log file unavailable: {e}");
            (None, None)
        }
    };
    let file_layer = writer.map(|w| {
        tracing_subscriber::fmt::layer()
            .with_writer(w)
            .with_ansi(false)
            .with_timer(LocalTime::rfc_3339())
            .with_filter(filter())
    });
    let console = cfg!(debug_assertions).then(|| tracing_subscriber::fmt::layer().with_filter(filter()));
    let _ = tracing_subscriber::registry().with(file_layer).with(console).try_init();
    guard
}

/// The release build targets AVX2 (section 17 notes); older CPUs would crash in whisper.cpp.
fn cpu_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        true
    }
}

/// A panic anywhere is logged and the active session's log is flushed (section 16).
fn log_panics(app: AppHandle) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        if let Some(st) = app.try_state()
            && let Some(guard) = st.recorder.try_lock()
            && let recorder::Phase::Recording(active) = &*guard
        {
            let _ = active.store.log.sync();
        }
        default(info);
    }));
}
