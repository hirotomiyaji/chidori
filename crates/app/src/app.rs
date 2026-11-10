//! The app: its state, what the core and the shell send it, and the window it draws. Each screen
//! is a module under `screens` with its own state, messages, update and view; this file routes
//! between them and holds what they share.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use chidori_core::audio::source::{AudioApp, Device};
use chidori_core::error::AppError;
use chidori_core::events::{Event, Notice, NoticeLevel, UiState};
use chidori_core::host::AppHandle;
use chidori_core::models::manager::ModelEntry;
use chidori_core::ops::{self, AppInfo, SessionView};
use chidori_core::session::projects::Project;
use chidori_core::settings::{Layout, Settings};
use gpui_kit::{
    AppContext as _, Bounds, Context, ExternalPaths, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Render, Styled, Subscription, Window, WindowAppearance, WindowBounds, WindowOptions, div, point, px, size,
};

use crate::bridge::{self, UiEvent};
use crate::model::line::LineHistory;
use crate::model::recording::Recording;
use crate::model::source::{AppLevels, SourceChoice};
use crate::model::transcript::Transcript;
use crate::native::{self, menu};
use crate::paint::{Point, Size};
use crate::screens;
use crate::task::{Cmd, Task};
use crate::theme::{self, p};
use crate::ui::{self, El, Ui};
use crate::{actions, i18n, icons};

pub const EXPANDED: Size = Size::new(420.0, 640.0);
const EXPANDED_MIN: Size = Size::new(360.0, 420.0);
pub const COMPACT: Size = Size::new(380.0, 170.0);
const COMPACT_MIN: Size = Size::new(340.0, 150.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Main,
    Settings,
    History,
    /// A past session, opened from 履歴.
    Session,
}

/// A section of 設定.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Audio,
    Transcription,
    Models,
    Screenshots,
    Hotkeys,
    Output,
}

/// The three places named in the title bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Main,
    History,
    Settings,
}

impl View {
    /// A past session sits under 履歴, the model list under 設定.
    pub fn place(self) -> Place {
        match self {
            View::History | View::Session => Place::History,
            View::Settings => Place::Settings,
            View::Main => Place::Main,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl From<NoticeLevel> for Level {
    fn from(l: NoticeLevel) -> Self {
        match l {
            NoticeLevel::Info => Level::Info,
            NoticeLevel::Warn => Level::Warn,
            NoticeLevel::Error => Level::Error,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub level: Level,
    pub at: Instant,
}

#[derive(Debug, Clone)]
pub struct Banner {
    pub id: u64,
    pub notice: Notice,
}

pub const TOAST_FOR: Duration = Duration::from_secs(2);

pub struct App {
    pub core: AppHandle,
    /// The window is open (commands that act on it wait for this).
    pub window: bool,
    pub hwnd: Option<u64>,
    /// The window is minimized or in the tray: nothing animates.
    pub hidden: bool,
    pub now: Instant,

    pub settings: Settings,
    pub info: AppInfo,
    /// The model catalog, for showing model names.
    pub models: Vec<ModelEntry>,
    pub rec: Recording,
    /// The live (or last recorded) session; the core's live events feed it.
    pub live: Transcript,
    pub line: LineHistory,
    pub source: SourceChoice,
    pub app_levels: AppLevels,
    pub projects: Vec<Project>,
    /// Counts changes to which session is in which project, so lists of sessions reload.
    pub projects_version: u64,

    pub view: View,
    pub settings_tab: SettingsTab,
    pub viewing_session: Option<String>,
    pub banners: Vec<Banner>,
    pub toasts: Vec<Toast>,
    /// The border flashes when a screenshot is taken.
    pub flash: Option<Instant>,
    /// A file is held over the window (FR-18).
    pub dropping: bool,
    next_id: u64,
    /// The layout the window was last sized for.
    shown_layout: Option<Layout>,

    pub main: screens::main_view::State,
    pub history: screens::history::State,
    pub viewer: screens::viewer::State,
    pub settings_ui: screens::settings::State,
    pub models_ui: screens::models::State,
    pub wizard: screens::wizard::State,
    pub dialogs: screens::dialogs::State,
}

#[derive(Debug, Clone)]
pub enum Message {
    Bridge(UiEvent),
    WindowOpened,
    Hwnd(u64),
    WindowEvent(WindowEvent),
    /// Windows switched between light (`false`) and dark.
    ThemeMode(bool),
    Tick(Instant),

    Go(View),
    GoSettings(SettingsTab),
    OpenSession(String),
    Win(WinAction),
    DismissBanner(u64),
    /// Runs a notice's action; the banner that offered it, if any, goes.
    NoticeAction(String, Option<u64>),
    Toast(String, Level),
    /// A command failed: a banner or a toast, by its error code (`fallback` is the toast's key).
    Failed(AppError, &'static str),
    /// Shows a native popup menu, at a point in the window or at the cursor.
    Popup(Vec<menu::Item>, Option<Point>),

    SettingsChanged(Box<(Settings, Settings)>),
    Info(Box<AppInfo>),
    Models(Vec<ModelEntry>),
    Projects(Vec<Project>),
    /// Which session is in which project changed.
    ProjectsChanged,
    Apps(Vec<AudioApp>),
    Devices(Vec<Device>),
    LiveLoaded(Box<SessionView>),
    /// Applies a partial settings object (validated in the core).
    UpdateSettings(serde_json::Value),

    Action(actions::Action),

    Titlebar(screens::titlebar::Msg),
    Main(screens::main_view::Msg),
    History(screens::history::Msg),
    Viewer(screens::viewer::Msg),
    SettingsUi(screens::settings::Msg),
    ModelsUi(screens::models::Msg),
    Wizard(screens::wizard::Msg),
    Dialog(screens::dialogs::Msg),
    Noop,
}

/// What happened to the window.
#[derive(Debug, Clone)]
pub enum WindowEvent {
    CloseRequested,
    /// Minimized or hidden (`true`), or back on screen.
    Hidden(bool),
    Moved(Point),
    Focused,
    FileDropped(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinAction {
    Minimize,
    Close,
    ToggleOnTop,
    ToggleLayout,
    Show,
    Hide,
    ToggleVisible,
}

/// Opens the window and runs the app until it quits. `on_start` runs first, inside the app.
pub fn run(on_start: impl FnOnce() + 'static) {
    let app = gpui_kit::application().with_assets(icons::Assets);
    app.run(move |cx| {
        on_start();
        gpui_kit::init(cx);
        let fonts = theme::FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect();
        if let Err(e) = cx.text_system().add_fonts(fonts) {
            tracing::warn!("loading the brand fonts: {e}");
        }
        let bounds = match saved_position() {
            Some(at) => Bounds::new(point(px(at.x), px(at.y)), size(px(EXPANDED.width), px(EXPANDED.height))),
            None => Bounds::centered(None, size(px(EXPANDED.width), px(EXPANDED.height)), cx),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            // Fixed when the window opens: the compact layout's, the smaller; the expanded one's
            // larger minimum is kept by `AppView::keep_limits`.
            window_min_size: Some(size(px(COMPACT_MIN.width), px(COMPACT_MIN.height))),
            is_resizable: true,
            // A driven test run stays out of sight, or at least out of the way.
            show: crate::drive::visible(),
            focus: !crate::drive::on(),
            app_id: Some("dev.yusai.chidori".into()),
            ..WindowOptions::default()
        };
        let opened = gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| AppView::new(window, cx)));
        if let Err(e) = opened {
            tracing::error!("opening the window: {e}");
            cx.quit();
        }
    });
}

/// `--show-in-captures`: the window shows in screenshots and screen recordings for this run,
/// whatever 設定 says, so the app itself can be captured (testing, docs) without changing the
/// user's setting.
fn show_in_captures() -> bool {
    static FLAG: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::args().skip(1).any(|a| a == "--show-in-captures"));
    *FLAG
}

/// Whether the window keeps out of screen captures: 設定's choice, unless the launch said otherwise.
fn capture_excluded(settings: &Settings) -> bool {
    settings.screenshot.exclude_self && !show_in_captures()
}

/// Where the window was last time (`window.json`), so it opens there again.
fn saved_position() -> Option<Point> {
    let app = bridge::core()?;
    let bytes = std::fs::read(app.paths().config.join("window.json")).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some(Point::new(v.get("x")?.as_f64()? as f32, v.get("y")?.as_f64()? as f32))
}

fn save_position(app: &AppHandle, at: Point) {
    let body = serde_json::json!({ "x": at.x, "y": at.y });
    let _ = std::fs::create_dir_all(&app.paths().config);
    if let Err(e) = std::fs::write(app.paths().config.join("window.json"), body.to_string()) {
        tracing::debug!("saving the window position: {e}");
    }
}

/// Runs a blocking core call off the UI thread and turns its result into a message.
pub fn call<T: Send + 'static>(
    app: &App,
    f: impl FnOnce(&AppHandle) -> T + Send + 'static,
    map: impl FnOnce(T) -> Message + Send + 'static,
) -> Task {
    let core = app.core.clone();
    Task::future(async move {
        match chidori_core::host::runtime().spawn_blocking(move || f(&core)).await {
            Ok(v) => map(v),
            Err(e) => {
                tracing::error!("core call panicked: {e}");
                Message::Noop
            }
        }
    })
}

impl App {
    pub fn new(core: AppHandle) -> (Self, Task) {
        let settings = ops::settings(&core);
        let info = ops::app_info(&core);
        let mut source = SourceChoice::default();
        source.init_from(&settings);
        let mut rec = Recording::default();
        rec.apply(&info.state);
        rec.engine = Some(info.engine.clone());
        let app = App {
            core,
            window: false,
            hwnd: None,
            hidden: false,
            now: Instant::now(),
            settings,
            info,
            models: Vec::new(),
            rec,
            live: Transcript::default(),
            line: LineHistory::default(),
            source,
            app_levels: AppLevels::default(),
            projects: Vec::new(),
            projects_version: 0,
            view: View::Main,
            settings_tab: SettingsTab::General,
            viewing_session: None,
            banners: Vec::new(),
            toasts: Vec::new(),
            flash: None,
            dropping: false,
            next_id: 1,
            shown_layout: None,
            main: Default::default(),
            history: Default::default(),
            viewer: Default::default(),
            settings_ui: Default::default(),
            models_ui: Default::default(),
            wizard: Default::default(),
            dialogs: Default::default(),
        };
        let follow = app.follow_session(app.rec.session_id.clone());
        let tasks = Task::batch([
            app.refresh_models(),
            app.refresh_projects(),
            app.refresh_devices(),
            follow,
            screens::dialogs::on_launch(&app),
        ]);
        (app, tasks)
    }

    pub fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn layout(&self) -> Layout {
        self.settings.window.layout
    }

    /// The setup wizard shows until setup is done and a model exists.
    pub fn needs_wizard(&self) -> bool {
        !self.info.setup_done || self.rec.state == UiState::NeedsModel
    }

    /// Only 録音 has a compact form; everywhere else the window is expanded.
    pub fn effective_layout(&self) -> Layout {
        if self.needs_wizard() || self.view != View::Main { Layout::Expanded } else { self.layout() }
    }

    pub fn compact(&self) -> bool {
        self.effective_layout() == Layout::Compact
    }

    pub fn refresh_models(&self) -> Task {
        call(self, ops::models_list, Message::Models)
    }

    pub fn refresh_info(&self) -> Task {
        call(self, |c| Box::new(ops::app_info(c)), Message::Info)
    }

    pub fn refresh_projects(&self) -> Task {
        call(self, ops::list_projects, Message::Projects)
    }

    pub fn refresh_apps(&mut self) -> Task {
        self.source.loading_apps = true;
        call(self, |_| ops::list_audio_apps().unwrap_or_default(), Message::Apps)
    }

    pub fn refresh_devices(&self) -> Task {
        call(self, |_| ops::list_mic_devices().unwrap_or_default(), Message::Devices)
    }

    /// Loads the transcript of a session the core reports as active.
    fn follow_session(&self, session_id: Option<String>) -> Task {
        let Some(id) = session_id else { return Task::none() };
        if self.live.session_id.as_deref() == Some(id.as_str()) {
            return Task::none();
        }
        call(
            self,
            move |c| ops::get_session(c, &id).ok(),
            |v| v.map_or(Message::Noop, |v| Message::LiveLoaded(Box::new(v))),
        )
    }

    /// Whether the live transcript shows the session being recorded, so live events belong to it.
    fn is_live(&self) -> bool {
        self.rec.session_id.is_some() && self.live.session_id == self.rec.session_id
    }

    pub fn go(&mut self, view: View) {
        if view == View::Settings && self.view != View::Settings {
            // 設定 opens at its first section unless another is asked for.
            self.settings_tab = SettingsTab::General;
        }
        self.view = view;
    }

    pub fn toast(&mut self, text: impl Into<String>, level: Level) {
        self.toasts.push(Toast { text: text.into(), level, at: self.now });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    pub fn push_banner(&mut self, notice: Notice) {
        // One banner per code; the newest replaces an older one.
        self.banners.retain(|b| b.notice.code != notice.code);
        let id = self.next_id();
        self.banners.push(Banner { id, notice });
        if self.banners.len() > 3 {
            self.banners.remove(0);
        }
    }

    pub fn update(&mut self, message: Message) -> Task {
        match message {
            Message::Bridge(event) => self.on_bridge(event),
            Message::WindowOpened => {
                self.window = true;
                let mut tasks = Vec::new();
                if self.settings.window.always_on_top {
                    tasks.push(Task::cmd(Cmd::OnTop(true)));
                }
                tasks.push(self.sync_layout());
                Task::batch(tasks)
            }
            Message::Hwnd(hwnd) => {
                self.hwnd = Some(hwnd);
                native::window::apply_chrome(hwnd);
                native::window::set_capture_excluded(hwnd, capture_excluded(&self.settings));
                if self.settings.window.always_on_top {
                    native::window::set_topmost(hwnd, true);
                }
                Task::none()
            }
            Message::WindowEvent(event) => self.on_window_event(event),
            Message::ThemeMode(dark) => {
                theme::set_dark(dark);
                Task::none()
            }
            Message::Tick(now) => {
                self.now = now;
                self.toasts.retain(|t| now.duration_since(t.at) < TOAST_FOR);
                if self.flash.is_some_and(|f| now.duration_since(f) > Duration::from_millis(400)) {
                    self.flash = None;
                }
                self.live.age(now);
                self.app_levels.advance(now);
                Task::batch([screens::main_view::tick(self), screens::viewer::tick(self)])
            }
            Message::Go(view) => {
                self.go(view);
                let refresh = match view {
                    View::History => screens::history::on_open(self),
                    View::Settings => Task::batch([self.refresh_info(), self.refresh_models()]),
                    _ => Task::none(),
                };
                Task::batch([refresh, self.sync_layout()])
            }
            Message::GoSettings(tab) => {
                self.view = View::Settings;
                self.settings_tab = tab;
                Task::batch([self.refresh_info(), self.refresh_models(), self.sync_layout()])
            }
            Message::OpenSession(id) => {
                self.viewing_session = Some(id.clone());
                self.view = View::Session;
                Task::batch([screens::viewer::open(self, id), self.sync_layout()])
            }
            Message::Win(action) => self.on_window_action(action),
            Message::DismissBanner(id) => {
                self.banners.retain(|b| b.id != id);
                Task::none()
            }
            Message::NoticeAction(command, banner) => {
                if let Some(id) = banner {
                    self.banners.retain(|b| b.id != id);
                }
                actions::run_notice_action(self, &command)
            }
            Message::Toast(text, level) => {
                self.toast(text, level);
                Task::none()
            }
            Message::Failed(err, fallback) => {
                actions::report_error(self, &err, fallback);
                Task::none()
            }
            Message::Popup(items, at) => {
                if !crate::drive::popup(&items)
                    && let Some(hwnd) = self.hwnd
                {
                    menu::popup(hwnd, &items, at);
                }
                Task::none()
            }
            Message::SettingsChanged(both) => {
                let (old, new) = *both;
                self.apply_settings(&old, &new);
                self.settings = new;
                Task::batch([self.sync_layout(), self.refresh_info()])
            }
            Message::UpdateSettings(partial) => {
                if let Some(hk) = partial.get("hotkeys") {
                    for k in ["toggle", "screenshot", "mark", "cut"] {
                        if let Some(acc) = hk.get(k).and_then(|v| v.as_str()).filter(|a| !a.trim().is_empty())
                            && let Err(e) = native::hotkeys::validate(acc)
                        {
                            self.toast(i18n::tf("failed", &[("detail", &e)]), Level::Error);
                            return Task::none();
                        }
                    }
                }
                call(self, move |c| Box::new(ops::set_settings(c, &partial)), Message::SettingsChanged)
            }
            Message::Info(info) => {
                self.info = *info;
                Task::none()
            }
            Message::Models(models) => {
                self.models = models;
                Task::none()
            }
            Message::Projects(projects) => {
                self.projects = projects;
                Task::none()
            }
            Message::ProjectsChanged => {
                self.projects_version += 1;
                let mut tasks = vec![self.refresh_projects(), screens::history::on_open(self)];
                // The session on screen shows its project in its header: reloaded with it.
                if let Some(id) = self.live.session_id.clone() {
                    tasks.push(call(
                        self,
                        move |c| ops::get_session(c, &id).ok(),
                        |v| v.map_or(Message::Noop, |v| Message::LiveLoaded(Box::new(v))),
                    ));
                }
                if self.view == View::Session
                    && let Some(id) = self.viewing_session.clone()
                {
                    tasks.push(Task::done(Message::OpenSession(id)));
                }
                Task::batch(tasks)
            }
            Message::Apps(apps) => {
                self.source.set_apps(apps);
                Task::none()
            }
            Message::Devices(devices) => {
                self.source.devices = devices;
                Task::none()
            }
            Message::LiveLoaded(view) => {
                self.live.load(*view);
                Task::none()
            }
            Message::Action(a) => actions::run(self, a),
            Message::Titlebar(m) => screens::titlebar::update(self, m),
            Message::Main(m) => screens::main_view::update(self, m),
            Message::History(m) => screens::history::update(self, m),
            Message::Viewer(m) => screens::viewer::update(self, m),
            Message::SettingsUi(m) => screens::settings::update(self, m),
            Message::ModelsUi(m) => screens::models::update(self, m),
            Message::Wizard(m) => screens::wizard::update(self, m),
            Message::Dialog(m) => screens::dialogs::update(self, m),
            Message::Noop => Task::none(),
        }
    }

    /// Settings that act on the shell: the locale, hotkeys, always on top, capture exclusion.
    fn apply_settings(&mut self, old: &Settings, new: &Settings) {
        if new.locale != old.locale {
            i18n::set_locale(new.locale);
            native::tray::update(self.rec.state, new.locale);
        }
        if new.hotkeys != old.hotkeys {
            native::hotkeys::register_all(&self.core);
        }
        if new.screenshot.exclude_self != old.screenshot.exclude_self
            && let Some(hwnd) = self.hwnd
        {
            native::window::set_capture_excluded(hwnd, capture_excluded(new));
        }
    }

    /// Sizes the window for the layout it shows, when that changed.
    pub fn sync_layout(&mut self) -> Task {
        if !self.window {
            return Task::none();
        }
        let layout = self.effective_layout();
        if self.shown_layout == Some(layout) {
            return Task::none();
        }
        self.shown_layout = Some(layout);
        let (size, min, resizable) = match layout {
            Layout::Compact => (COMPACT, COMPACT_MIN, false),
            Layout::Expanded => (EXPANDED, EXPANDED_MIN, true),
        };
        Task::cmd(Cmd::Size { w: size.width, h: size.height, min_w: min.width, min_h: min.height, resizable })
    }

    fn on_bridge(&mut self, event: UiEvent) -> Task {
        match event {
            UiEvent::Core(e) => self.on_core(e),
            UiEvent::TrayClick => self.on_window_action(WinAction::ToggleVisible),
            UiEvent::SecondInstance => self.on_window_action(WinAction::Show),
            UiEvent::Exit => crate::task::exit(),
            UiEvent::Menu(id) => {
                tracing::debug!("menu pick {id}");
                self.on_menu(id)
            }
        }
    }

    /// A pick in a native menu, routed by its id's prefix to the screen that opened it.
    fn on_menu(&mut self, id: String) -> Task {
        let (owner, rest) = id.split_once(':').unwrap_or((id.as_str(), ""));
        let rest = rest.to_string();
        match owner {
            "tray" => match id.as_str() {
                native::tray::TOGGLE => actions::run(self, actions::Action::ToggleRecording),
                native::tray::SHOT => actions::run(self, actions::Action::Screenshot),
                native::tray::SHOW => self.on_window_action(WinAction::Show),
                native::tray::QUIT => actions::request_quit(self),
                _ => Task::none(),
            },
            "action" => actions::on_menu(self, &rest),
            "titlebar" => screens::titlebar::on_menu(self, &rest),
            "main" => screens::main_view::on_menu(self, &rest),
            "history" => screens::history::on_menu(self, &rest),
            "viewer" => screens::viewer::on_menu(self, &rest),
            "settings" => screens::settings::on_menu(self, &rest),
            "dialog" => screens::dialogs::on_menu(self, &rest),
            _ => Task::none(),
        }
    }

    fn on_core(&mut self, event: Event) -> Task {
        match event {
            Event::State(p) => {
                let before = self.rec.state;
                // The line keeps its own history: a new recording starts an empty one, a resume
                // (or a continued session, FR-08) leaves a gap.
                if p.state == UiState::Recording
                    && let Some(id) = &p.session_id
                {
                    let again = self.line.session_id.as_deref() == Some(id.as_str());
                    self.line.start(id);
                    if again && before != UiState::Recording {
                        self.line.gap();
                    }
                }
                self.rec.apply(&p);
                native::tray::update(p.state, self.settings.locale);
                let needs_info = (before == UiState::NeedsModel) != (p.state == UiState::NeedsModel);
                let mut tasks = vec![self.follow_session(p.session_id.clone()), self.sync_layout()];
                if needs_info {
                    tasks.push(self.refresh_info());
                }
                Task::batch(tasks)
            }
            Event::Levels(levels) => {
                if self.rec.state == UiState::Recording {
                    self.line.push(&levels);
                }
                screens::settings::on_levels(self, &levels);
                screens::wizard::on_levels(self, &levels);
                Task::none()
            }
            Event::AppLevels(levels) => {
                self.app_levels.hear(&levels, self.now);
                Task::none()
            }
            Event::Segment(seg) => {
                if self.is_live() {
                    self.live.add_segment(seg);
                }
                Task::none()
            }
            Event::Partial(p) => {
                if self.is_live() {
                    self.live.set_partial(p);
                }
                Task::none()
            }
            Event::SegmentRemoved(p) => {
                if self.is_live() {
                    self.live.remove_item(&p.id);
                }
                Task::none()
            }
            Event::SegmentUpdated(seg) => {
                if self.is_live() {
                    self.live.update_segment(seg);
                }
                Task::none()
            }
            Event::Screenshot(p) => {
                if self.is_live() {
                    self.live.add_screenshot(p);
                    self.line.shot();
                }
                Task::none()
            }
            Event::Marker(p) => {
                if self.is_live() {
                    self.live.add_marker(p);
                }
                Task::none()
            }
            Event::Lag(l) => {
                self.rec.lag = Some(l);
                Task::none()
            }
            Event::Finishing(p) => {
                self.rec.finishing = Some(p);
                Task::none()
            }
            Event::ModelStatus(status) => {
                self.rec.engine = Some(status);
                Task::batch([self.refresh_info(), self.refresh_models()])
            }
            Event::ModelDownload(p) => {
                Task::batch([screens::models::on_download(self, &p), screens::wizard::on_download(self, &p)])
            }
            Event::SessionSaved(p) => {
                self.rec.last_saved_folder = Some(p.folder.clone());
                self.toast(i18n::t("saved"), Level::Info);
                let id = p.session_id.clone();
                if self.live.session_id.as_deref() == Some(id.as_str()) {
                    call(
                        self,
                        move |c| ops::get_session(c, &id).ok(),
                        |v| v.map_or(Message::Noop, |v| Message::LiveLoaded(Box::new(v))),
                    )
                } else {
                    Task::none()
                }
            }
            Event::Notice(n) => {
                actions::show_notice(self, n);
                Task::none()
            }
        }
    }

    fn on_window_event(&mut self, event: WindowEvent) -> Task {
        match event {
            WindowEvent::CloseRequested => self.on_window_action(WinAction::Close),
            WindowEvent::Hidden(hidden) => {
                self.set_hidden(hidden);
                Task::none()
            }
            WindowEvent::Moved(at) => {
                save_position(&self.core, at);
                Task::none()
            }
            WindowEvent::Focused => {
                self.set_hidden(false);
                Task::none()
            }
            WindowEvent::FileDropped(path) => {
                let ok = self.can_drop();
                self.dropping = false;
                if ok { actions::transcribe_file(self, Some(path)) } else { Task::none() }
            }
        }
    }

    /// A dropped file is transcribed only when nothing records and setup is done (FR-18).
    pub fn can_drop(&self) -> bool {
        self.rec.state == UiState::Ready && !self.needs_wizard()
    }

    fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        chidori_core::host::set_ui_hidden(hidden);
    }

    pub fn on_window_action(&mut self, action: WinAction) -> Task {
        if !self.window {
            return Task::none();
        }
        match action {
            WinAction::Minimize => {
                self.set_hidden(true);
                Task::cmd(Cmd::Minimize)
            }
            WinAction::Close => {
                // Closing while recording or finishing hides to the tray; otherwise the app quits.
                if self.rec.busy() {
                    native::notify::show(&chidori_core::host::os_text(self.settings.locale, "hiddenToTray", ""));
                    self.on_window_action(WinAction::Hide)
                } else {
                    crate::task::exit()
                }
            }
            WinAction::Hide => {
                self.set_hidden(true);
                Task::cmd(Cmd::Hide)
            }
            WinAction::Show => {
                self.set_hidden(false);
                Task::cmd(Cmd::Show)
            }
            WinAction::ToggleVisible => {
                if self.hidden {
                    self.on_window_action(WinAction::Show)
                } else {
                    self.on_window_action(WinAction::Hide)
                }
            }
            WinAction::ToggleOnTop => {
                let on = !self.settings.window.always_on_top;
                Task::batch([
                    Task::cmd(Cmd::OnTop(on)),
                    Task::done(Message::UpdateSettings(serde_json::json!({ "window": { "alwaysOnTop": on } }))),
                ])
            }
            WinAction::ToggleLayout => {
                // Only 録音 has a compact form: shrinking from 履歴 or 設定 goes to 録音.
                let compact = self.view == View::Main && self.layout() == Layout::Compact;
                let next = if compact { Layout::Expanded } else { Layout::Compact };
                self.view = View::Main;
                self.settings.window.layout = next;
                let core = self.core.clone();
                chidori_core::host::runtime().spawn_blocking(move || ops::set_layout(&core, next));
                self.sync_layout()
            }
        }
    }

    /// Messages sent again and again while something waits on the clock, each with its interval;
    /// the tick that animates comes from `tick_every`.
    fn timers(&self) -> Vec<(Duration, Message)> {
        let mut t = Vec::new();
        t.extend(screens::main_view::timers(self));
        t.extend(screens::viewer::timers(self));
        t
    }

    /// A key pressed anywhere in the window, for the screens that answer one.
    fn on_key(&self, key: &ui::KeyPress) -> Vec<Message> {
        let mut out = Vec::new();
        out.extend(screens::dialogs::on_key(self, key));
        if screens::dialogs::is_open(self) {
            return out;
        }
        out.extend(screens::settings::on_key(self, key));
        out.extend(screens::main_view::on_key(self, key));
        out.extend(screens::viewer::on_key(self, key));
        out.extend(screens::history::on_key(self, key));
        out
    }

    /// How often to redraw for what moves, or never: nothing moves while the window is hidden.
    fn tick_every(&self) -> Option<Duration> {
        if self.hidden {
            return None;
        }
        // The recording's time and sound step ten times a second; what moves on the jello asks
        // GPUI for its own frames.
        if screens::main_view::stepping(self) {
            return Some(Duration::from_millis(100));
        }
        let waves = self.app_levels.moving(self.now);
        let spinning = screens::spinning(self);
        if waves || spinning || !self.live.fresh.is_empty() {
            return Some(Duration::from_millis(50));
        }
        if !self.toasts.is_empty() || self.flash.is_some() {
            return Some(Duration::from_millis(100));
        }
        None
    }

    pub fn view(&self, ui: &mut Ui) -> El {
        let body: El = if self.needs_wizard() {
            screens::wizard::view(self)
        } else {
            match self.view {
                View::Main => screens::main_view::view(self, ui),
                View::Settings => screens::settings::view(self, ui),
                View::History => screens::history::view(self, ui),
                View::Session => screens::viewer::view(self, ui),
            }
        };
        // The compact window draws its own head, with what is recorded.
        let mut page = div().size_full().flex().flex_col();
        if !self.compact() {
            page = page.child(screens::titlebar::view(self));
        }
        let page = page.child(div().flex_1().min_h_0().w_full().flex().flex_col().child(body));
        let p = p();
        // A screenshot rings the window in ink for a moment.
        let mut root = div()
            .relative()
            .size_full()
            .bg(p.ground)
            .text_color(p.ink)
            .font_family(theme::SANS)
            .text_size(px(theme::tokens::BODY.size))
            .line_height(px(theme::tokens::BODY.line_height))
            .child(page);
        if self.flash.is_some() {
            root = root.border_2().border_color(p.ink);
        }
        if self.dropping {
            root = root.child(screens::notices::drop_overlay());
        }
        root = root.child(screens::notices::toasts(self));
        if let Some(d) = screens::dialogs::view(self, ui) {
            root = root.child(d);
        }
        root.into_any_element()
    }
}

/// The window's one view: the app, and what GPUI needs to run it.
pub struct AppView {
    pub app: App,
    focus: FocusHandle,
    /// The animation tick and its interval.
    ticker: Option<(Duration, gpui_kit::Task<()>)>,
    /// Running timers, by what they send.
    timers: Vec<(String, gpui_kit::Task<()>)>,
    /// The size, minimum and resizability the layout asks of the window.
    limits: Option<(Size, Size, bool)>,
    last_origin: Option<Point>,
    _subs: Vec<Subscription>,
}

impl AppView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let core = bridge::core().expect("core set before the window opens").clone();
        let dark = matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark);
        theme::set_dark(dark);
        theme::apply_to_kit(cx);
        let (app, task) = App::new(core);
        ui::install(window.window_handle(), cx.weak_entity());
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut subs = vec![
            cx.observe_window_appearance(window, |_, window, cx| {
                let dark = matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark);
                theme::set_dark(dark);
                theme::apply_to_kit(cx);
                ui::send(Message::ThemeMode(dark), cx);
            }),
            cx.observe_window_bounds(window, |v, window, cx| v.on_bounds(window, cx)),
            cx.observe_window_activation(window, |v, window, cx| {
                if window.is_window_active() {
                    ui::send(Message::WindowEvent(WindowEvent::Focused), cx);
                }
                v.check_hidden(cx);
            }),
            cx.observe_keystrokes(|v, ev, _, cx| {
                let key = ui::KeyPress::from(&ev.keystroke);
                for m in v.app.on_key(&key) {
                    ui::send(m, cx);
                }
            }),
        ];
        subs.push(cx.observe_window_visibility(window, |v, _, _, cx| v.check_hidden(cx)));
        window.on_window_should_close(cx, |_, cx| {
            ui::send(Message::WindowEvent(WindowEvent::CloseRequested), cx);
            false
        });
        // The core's events, the tray, menus and hotkeys arrive on one channel.
        if let Some(mut rx) = bridge::take_receiver() {
            cx.spawn(async move |_, cx| {
                while let Some(e) = rx.recv().await {
                    cx.update(|cx| ui::send(Message::Bridge(e), cx));
                }
            })
            .detach();
        }
        #[cfg(feature = "drive")]
        {
            let handle = window.window_handle();
            cx.spawn(async move |_, cx| crate::drive::serve(handle, cx).await).detach();
        }
        let hwnd = hwnd_of(window);
        ui::run(task, cx);
        if let Some(h) = hwnd {
            ui::send(Message::Hwnd(h), cx);
        }
        ui::send(Message::WindowOpened, cx);
        Self { app, focus, ticker: None, timers: Vec::new(), limits: None, last_origin: None, _subs: subs }
    }

    /// Carries out a window command an update asked for.
    pub fn apply(&mut self, cmd: Cmd, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            Cmd::Focus(key) => ui::focus(&key, window, cx),
            Cmd::Size { w, h, min_w, min_h, resizable } => {
                self.limits = Some((Size::new(w, h), Size::new(min_w, min_h), resizable));
                window.resize(size(px(w), px(h)));
            }
            Cmd::OnTop(on) => {
                if let Some(h) = self.app.hwnd {
                    native::window::set_topmost(h, on);
                }
            }
            Cmd::Minimize => window.minimize_window(),
            // A driven run's window is shown or not for the whole run.
            Cmd::Hide | Cmd::Show if crate::drive::on() => {}
            Cmd::Hide => {
                if let Some(h) = self.app.hwnd {
                    native::window::set_visible(h, false);
                }
            }
            Cmd::Show => {
                if let Some(h) = self.app.hwnd {
                    native::window::set_visible(h, true);
                }
                window.activate_window();
            }
            Cmd::Exit => cx.quit(),
        }
    }

    /// After a batch of messages: start or stop what runs on the clock.
    pub fn after_update(&mut self, cx: &mut Context<Self>) {
        screens::housekeeping(&self.app);
        let every = self.app.tick_every();
        if self.ticker.as_ref().map(|t| t.0) != every {
            self.ticker = every.map(|d| (d, repeat(d, || Message::Tick(Instant::now()), cx)));
        }
        let wanted = self.app.timers();
        self.timers.retain(|(k, _)| wanted.iter().any(|(d, m)| &timer_key(*d, m) == k));
        for (d, m) in wanted {
            let k = timer_key(d, &m);
            if !self.timers.iter().any(|(t, _)| *t == k) {
                self.timers.push((k, repeat(d, move || m.clone(), cx)));
            }
        }
    }

    fn on_bounds(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Saved as GPUI saves a window for reopening: its live origin is the client area's, which
        // opened 4 px higher on every launch.
        let b = window.window_bounds().get_bounds();
        let origin = Point::new(f32::from(b.origin.x), f32::from(b.origin.y));
        if self.last_origin != Some(origin) {
            self.last_origin = Some(origin);
            ui::send(Message::WindowEvent(WindowEvent::Moved(origin)), cx);
        }
        self.keep_limits(window);
        self.check_hidden(cx);
    }

    /// GPUI fixes a window's minimum size and resizability when it opens; the layout's own are
    /// kept here by sizing the window back.
    fn keep_limits(&mut self, window: &mut Window) {
        let Some((want, min, resizable)) = self.limits else { return };
        if self.app.hidden {
            return;
        }
        let b = window.bounds();
        let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
        let target = if resizable { Size::new(w.max(min.width), h.max(min.height)) } else { want };
        if (target.width - w).abs() > 0.5 || (target.height - h).abs() > 0.5 {
            window.resize(size(px(target.width), px(target.height)));
        }
    }

    fn check_hidden(&mut self, cx: &mut Context<Self>) {
        let Some(h) = self.app.hwnd else { return };
        let hidden = native::window::is_minimized(h);
        if hidden != self.app.hidden {
            ui::send(Message::WindowEvent(WindowEvent::Hidden(hidden)), cx);
        }
    }
}

fn timer_key(d: Duration, m: &Message) -> String {
    format!("{}:{m:?}", d.as_millis())
}

/// Sends a message every `every` until the returned task is dropped.
fn repeat(every: Duration, msg: impl Fn() -> Message + 'static, cx: &mut Context<AppView>) -> gpui_kit::Task<()> {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(every).await;
            if this.update(cx, |_, cx| ui::send(msg(), cx)).is_err() {
                break;
            }
        }
    })
}

/// The native handle of the window (HWND, or NSView on macOS), for what only the OS can do.
fn hwnd_of(window: &Window) -> Option<u64> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as u64),
        RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr() as usize as u64),
        _ => None,
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::begin_frame();
        self.app.dropping = cx.has_active_drag() && self.app.can_drop();
        let view = {
            let mut ui = Ui { window, cx };
            self.app.view(&mut ui)
        };
        div()
            .id("chidori")
            .size_full()
            .track_focus(&self.focus)
            .on_drop(|paths: &ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().first() {
                    ui::send(Message::WindowEvent(WindowEvent::FileDropped(path.clone())), cx);
                }
            })
            .child(view)
    }
}

/// An empty element, for a part with nothing to show.
pub fn nothing() -> El {
    div().into_any_element()
}

/// A path for a screenshot of a session (`images/…` under its folder).
pub fn session_file(folder: &std::path::Path, rel: &str) -> PathBuf {
    folder.join(rel)
}
