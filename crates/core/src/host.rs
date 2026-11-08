//! What the core needs from the app around it: its folders, where events go, the shared state,
//! and the few things only the shell can do (the tray, the clipboard, notifications, quitting).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::events::{Event, EventSink, UiState};
use crate::settings::Locale;
use crate::state::AppState;

/// The app's identifier; its folders are named after it.
pub const IDENTIFIER: &str = "dev.yusai.chidori";

/// Where the app keeps its files. The folders are the ones the Tauri build used, so settings,
/// models and logs carry over.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Settings and state (`%APPDATA%\dev.yusai.chidori`).
    pub config: PathBuf,
    /// Models (`%LOCALAPPDATA%\dev.yusai.chidori`).
    pub local_data: PathBuf,
    pub logs: PathBuf,
    /// Files shipped next to the exe, or in a Mac bundle's Resources (the benchmark clip, licences).
    pub resources: PathBuf,
    pub documents: PathBuf,
}

impl Paths {
    pub fn system() -> Self {
        let config = dirs::config_dir().unwrap_or_default().join(IDENTIFIER);
        let local_data = dirs::data_local_dir().unwrap_or_default().join(IDENTIFIER);
        let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(PathBuf::from)).unwrap_or_default();
        // In a macOS bundle the exe is in Contents/MacOS and its files in Contents/Resources.
        let bundled = exe_dir.ends_with("Contents/MacOS").then(|| exe_dir.with_file_name("Resources"));
        let resources = bundled.filter(|r| cfg!(target_os = "macos") && r.is_dir()).unwrap_or(exe_dir);
        Self {
            logs: local_data.join("logs"),
            config,
            local_data,
            resources,
            documents: dirs::document_dir().unwrap_or_default(),
        }
    }
}

/// What only the app's shell can do.
pub trait Shell: Send + Sync {
    /// The recording state changed; the tray icon follows it.
    fn state_changed(&self, _state: UiState) {}
    /// Whether the main window is out of sight (minimized or in the tray).
    fn window_hidden(&self) -> bool {
        false
    }
    /// A silent system notification, for when the window is out of sight.
    fn notify(&self, _text: &str) {}
    fn write_clipboard(&self, text: &str) -> anyhow::Result<()>;
    /// Quits the app (after a recording finished saving, when the user asked to quit).
    fn exit(&self) {}
}

struct Inner {
    paths: Paths,
    version: String,
    events: Arc<dyn EventSink>,
    shell: Arc<dyn Shell>,
    state: OnceLock<AppState>,
}

/// The core's handle on the app, cheap to clone and shared by every thread.
#[derive(Clone)]
pub struct AppHandle(Arc<Inner>);

impl AppHandle {
    pub fn new(paths: Paths, version: &str, events: Arc<dyn EventSink>, shell: Arc<dyn Shell>) -> Self {
        Self(Arc::new(Inner { paths, version: version.into(), events, shell, state: OnceLock::new() }))
    }

    /// Installs the state, once, after the ASR worker exists.
    pub fn manage(&self, state: AppState) {
        if self.0.state.set(state).is_err() {
            tracing::error!("app state installed twice");
        }
    }

    pub fn state(&self) -> &AppState {
        self.0.state.get().expect("app state used before it was installed")
    }

    pub fn try_state(&self) -> Option<&AppState> {
        self.0.state.get()
    }

    pub fn paths(&self) -> &Paths {
        &self.0.paths
    }

    pub fn version(&self) -> &str {
        &self.0.version
    }

    pub fn shell(&self) -> &dyn Shell {
        self.0.shell.as_ref()
    }

    pub fn events(&self) -> Arc<dyn EventSink> {
        self.0.events.clone()
    }
}

impl EventSink for AppHandle {
    fn emit(&self, event: Event) {
        self.0.events.emit(event);
    }
}

/// The core's async runtime: model downloads and the update check run on it.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("chidori-async")
            .enable_all()
            .build()
            .expect("starting the async runtime")
    })
}

/// Text for a system notification, which shows outside the UI and its translations.
pub fn os_text(locale: Locale, key: &str, param: &str) -> String {
    match (locale, key) {
        (Locale::Ja, "shotAdded") => format!("スクショを挿入しました {param}"),
        (Locale::En, "shotAdded") => format!("Screenshot added {param}"),
        (Locale::Ja, "hiddenToTray") => "トレイで録音を続けています".to_string(),
        (Locale::En, "hiddenToTray") => "Still recording in the tray".to_string(),
        (_, other) => other.to_string(),
    }
}

static UI_HIDDEN: AtomicBool = AtomicBool::new(false);

/// Set by the app when its window is minimized or in the tray, so nothing draws for no one.
pub fn set_ui_hidden(hidden: bool) {
    UI_HIDDEN.store(hidden, Ordering::Relaxed);
}

pub fn ui_hidden() -> bool {
    UI_HIDDEN.load(Ordering::Relaxed)
}
