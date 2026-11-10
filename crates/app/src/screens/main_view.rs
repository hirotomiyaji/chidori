//! 録音: what to listen to and 開始 before a recording; the ink card, the transcript and the bar
//! while recording; and the session that just finished, its white card over the transcript
//! (DECISIONS → UI, "The card and the bar"). The compact window keeps three rows: what is
//! recorded, the latest line, and the time with 停止.

#[path = "main/card.rs"]
pub mod card;
#[path = "main/playback.rs"]
pub mod playback;
#[path = "main/rows.rs"]
pub mod rows;
#[path = "main/scrollbar.rs"]
pub mod scrollbar;
#[path = "main/source_picker.rs"]
pub mod source_picker;
#[path = "main/transcript.rs"]
pub mod transcript;

use std::time::Duration;

use chidori_core::error::AppError;
use chidori_core::events::UiState;
use chidori_core::ops;
use chidori_core::screenshot::WindowInfo;
use chidori_core::settings::{Settings, SourceMode};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{InteractiveElement, IntoElement, ParentElement, Styled, WindowControlArea, div, px};

use self::card::{CardMsg, CardState, SessionCtx};
use self::playback::{Player, Status};
use self::source_picker::PickMsg;
use self::transcript::{Ctx, ListMsg, ListState};
use crate::app::{App, Level, Message, View, call};
use crate::i18n::t;
use crate::icons::{Icon, icon};
use crate::model::format::sources_text;
use crate::model::source::ChosenApp;
use crate::motion::{self, Look};
use crate::screens::notices;
use crate::screens::titlebar;
use crate::task::Task;
use crate::theme::{GRID, Ink, R_PANEL, R_ROW, g, line, p, plain, tokens};
use crate::ui::{El, KeyPress, Ui, clickable, uid};
use crate::widgets::{col, fill, row, with_tip};

/// What the main view shows: nothing yet, a recording, or the session that just finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Start,
    Live,
    Review,
}

pub fn phase(app: &App) -> Phase {
    if app.rec.busy() {
        Phase::Live
    } else if app.live.session_id.is_some() {
        Phase::Review
    } else {
        Phase::Start
    }
}

/// A source choice, kept to switch back to when a switch fails.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    mode: SourceMode,
    app: Option<ChosenApp>,
    include_mic: bool,
    mic: Option<String>,
}

impl Choice {
    fn of(app: &App) -> Self {
        let s = &app.source;
        Self { mode: s.mode, app: s.app.clone(), include_mic: s.include_mic, mic: s.mic_device_id.clone() }
    }

    /// The choice as the recorder compares it: which app, not which of its processes.
    fn key(&self) -> (SourceMode, Option<String>, bool, Option<String>) {
        let app = if self.mode == SourceMode::App { self.app.as_ref().map(|a| a.exe.to_lowercase()) } else { None };
        (self.mode, app, self.mode != SourceMode::Mic && self.include_mic, self.mic.clone())
    }

    fn restore(self, app: &mut App) {
        let s = &mut app.source;
        s.mode = self.mode;
        s.app = self.app;
        s.include_mic = self.include_mic;
        s.mic_device_id = self.mic;
    }
}

pub struct State {
    pub list: ListState,
    pub card: CardState,
    pub player: Player,
    /// Where the sound being played is.
    pub playing_ms: Option<u64>,
    /// The windows the camera's menu last listed.
    windows: Vec<WindowInfo>,
    /// The recording whose source choice was taken from the settings.
    live_session: Option<String>,
    /// The choice the recording records now.
    applied: Option<Choice>,
    /// The session the list shows, and the phase it was last seen in.
    list_session: Option<String>,
    last_phase: Option<Phase>,
    /// The recording card comes in from where 開始 was: set when a recording starts in view.
    enter_card: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            list: ListState::new("main"),
            card: CardState::default(),
            player: Player::default(),
            playing_ms: None,
            windows: Vec::new(),
            live_session: None,
            applied: None,
            list_session: None,
            last_phase: None,
            enter_card: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Pick(PickMsg),
    List(ListMsg),
    Card(CardMsg),
    /// The settings at the start of a recording, which may have come from the hotkey.
    LiveSettings(Box<Settings>),
    Switched(Result<(), AppError>, Box<Choice>),
    PlayTick,
    Escape,
}

fn wrap_list(m: ListMsg) -> Message {
    Message::Main(Msg::List(m))
}

fn wrap_card(m: CardMsg) -> Message {
    Message::Main(Msg::Card(m))
}

pub fn update(app: &mut App, msg: Msg) -> Task {
    match msg {
        Msg::Pick(m) => {
            let task = source_picker::update(app, m);
            let watch = sync_watch(app);
            Task::batch([task, watch])
        }
        Msg::List(ListMsg::PlayFrom(ms)) => {
            play_from(app, ms);
            Task::none()
        }
        Msg::List(m) => {
            let core = app.core.clone();
            transcript::update(&core, &mut app.main.list, &mut app.live, m, wrap_list)
        }
        Msg::Card(CardMsg::Seek(ms)) => {
            if app.main.player.status() != Status::Idle || !audio_files(app).is_empty() {
                play_from(app, ms);
            }
            update(app, Msg::List(ListMsg::Seek(ms)))
        }
        Msg::Card(CardMsg::Play) => {
            toggle_play(app);
            Task::none()
        }
        Msg::Card(CardMsg::Export(id)) => {
            crate::screens::dialogs::ask_export(app, id);
            Task::none()
        }
        Msg::Card(CardMsg::ShotMenu) => call(
            app,
            |c| ops::list_windows(c).unwrap_or_default(),
            |w| Message::Main(Msg::Card(CardMsg::ShotWindows(w))),
        ),
        Msg::Card(CardMsg::ShotWindows(windows)) => {
            let items = card::shot_menu(app, windows.clone());
            app.main.windows = windows;
            Task::done(Message::Popup(items, None))
        }
        Msg::LiveSettings(settings) => {
            app.source.init_from(&settings);
            app.main.applied = Some(Choice::of(app));
            source_picker::refresh(app)
        }
        Msg::Switched(Ok(()), _) => {
            app.toast(t("sourceSwitched"), Level::Info);
            Task::none()
        }
        Msg::Switched(Err(e), before) => {
            crate::actions::report_error(app, &e, "failed");
            app.main.applied = Some((*before).clone());
            before.restore(app);
            Task::none()
        }
        Msg::PlayTick => play_tick(app),
        Msg::Escape => update(app, Msg::List(ListMsg::Escape)),
    }
}

/// A pick in the camera's menu: one of the three targets (the setting itself, remembered), or a
/// window for the rest of this recording (FR-34).
pub fn on_menu(app: &mut App, rest: &str) -> Task {
    if let Some(target) = rest.strip_prefix("shot-target:") {
        let picked = app.rec.shot_window.is_some();
        let current = serde_json::to_value(app.settings.screenshot.target).ok();
        let clear = if picked {
            call(
                app,
                |c| ops::set_shot_window(c, None),
                |r| r.err().map_or(Message::Noop, |e| Message::Failed(e, "failed")),
            )
        } else {
            Task::none()
        };
        let set = if current.as_ref().and_then(|v| v.as_str()) != Some(target) {
            Task::done(Message::UpdateSettings(serde_json::json!({ "screenshot": { "target": target } })))
        } else {
            Task::none()
        };
        return Task::batch([clear, set]);
    }
    if let Some(id) = rest.strip_prefix("shot-window:")
        && let Some(w) = app.main.windows.iter().find(|w| w.id.to_string() == id).cloned()
    {
        return call(
            app,
            move |c| ops::set_shot_window(c, Some(w)),
            |r| r.err().map_or(Message::Noop, |e| Message::Failed(e, "failed")),
        );
    }
    Task::none()
}

/// The source choice changed (a menu pick): while recording it becomes a switch (FR-17); if the
/// new source cannot start, the recording keeps the old one and the choice goes back to it.
pub fn source_changed(app: &mut App) -> Task {
    if !matches!(app.rec.state, UiState::Recording | UiState::Paused) {
        return Task::none();
    }
    let Some(before) = app.main.applied.clone() else { return Task::none() };
    let now = Choice::of(app);
    if now.key() == before.key() || (now.mode == SourceMode::App && now.app.is_none()) {
        return Task::none();
    }
    app.main.applied = Some(now);
    let cfg = app.source.config();
    let before = Box::new(before);
    call(app, move |c| ops::switch_source(c, cfg), move |r| Message::Main(Msg::Switched(r, before)))
}

/// The apps whose sound is read: every app with sound while the start screen lists them.
fn sync_watch(app: &mut App) -> Task {
    let start = app.view == View::Main && !app.hidden && !app.compact() && phase(app) == Phase::Start;
    let pids = if start { source_picker::shown_pids(app, true) } else { Vec::new() };
    source_picker::sync_watch(app, pids)
}

fn audio_files(app: &App) -> Vec<(std::path::PathBuf, u64)> {
    match &app.live.folder {
        Some(f) => app.live.audio.iter().map(|a| (crate::app::session_file(f, &a.file), a.start_ms)).collect(),
        None => Vec::new(),
    }
}

fn play_from(app: &mut App, ms: u64) {
    let files = audio_files(app);
    if files.is_empty() || phase(app) != Phase::Review {
        return;
    }
    app.main.player.play_from(files, ms);
    app.main.playing_ms = Some(ms);
}

fn toggle_play(app: &mut App) {
    match app.main.player.status() {
        Status::Playing => app.main.player.pause(),
        Status::Paused => app.main.player.resume(),
        _ => play_from(app, 0),
    }
}

/// Four times a second while playing: where the sound is, and the transcript keeping up with it.
fn play_tick(app: &mut App) -> Task {
    // Leaving the session (or the review) stops it and lets go of the file.
    if app.view != View::Main || phase(app) != Phase::Review {
        app.main.player.stop();
        app.main.playing_ms = None;
        app.main.list.playing = None;
        return Task::none();
    }
    let status = app.main.player.status();
    if status == Status::Failed {
        app.toast(t("audioMissing"), Level::Warn);
    }
    if matches!(status, Status::Ended | Status::Failed) {
        app.main.player.reset();
    }
    app.main.playing_ms = app.main.player.position();
    let rows = app.main.list.rows(&app.live);
    let key = app.main.playing_ms.and_then(|ms| rows::playing_key(&app.live, &rows, ms));
    app.main.list.follow_playing(&app.live, key)
}

pub fn tick(app: &mut App) -> Task {
    let mut tasks = vec![sync_watch(app)];
    // A recording takes its source choice from the settings first, without switching: it may
    // have been started by the hotkey with the last-used source, which the picker never showed.
    let recording = matches!(app.rec.state, UiState::Recording | UiState::Paused);
    if recording && app.main.live_session != app.rec.session_id {
        app.main.live_session = app.rec.session_id.clone();
        app.main.applied = None;
        tasks.push(call(app, |c| Box::new(ops::settings(c)), |s| Message::Main(Msg::LiveSettings(s))));
    } else if !recording {
        app.main.live_session = None;
        app.main.applied = None;
    }
    // A new session on screen starts its list afresh; a finished one opens at its top.
    if app.main.list_session != app.live.session_id {
        app.main.list_session = app.live.session_id.clone();
        app.main.list.reset();
    }
    let ph = phase(app);
    if ph != Phase::Live || app.view != View::Main {
        app.main.enter_card = false;
    }
    if app.view == View::Main && app.main.last_phase != Some(ph) {
        let was = app.main.last_phase.replace(ph);
        match ph {
            Phase::Review if was.is_some() => app.main.list.stay(),
            Phase::Live => {
                app.main.list.forget();
                app.main.enter_card = was == Some(Phase::Start);
            }
            _ => {}
        }
        if ph != Phase::Review {
            app.main.player.stop();
            app.main.playing_ms = None;
        }
    }
    Task::batch(tasks)
}

/// While recording, the time and the sound step on the clock.
pub fn stepping(app: &App) -> bool {
    app.view == View::Main && !app.needs_wizard() && app.rec.state == UiState::Recording
}

pub fn spinning(app: &App) -> bool {
    if app.view != View::Main {
        return false;
    }
    let ph = phase(app);
    app.rec.state == UiState::Finishing
        || (ph == Phase::Live && transcript::writing(app))
        || (ph == Phase::Start && app.source.loading_apps)
        || (ph == Phase::Start
            && app.rec.engine.as_ref().is_some_and(|e| e.state == chidori_core::asr::worker::EngineState::Loading))
}

/// Keeps the list fresh for when the view shows again.
pub fn housekeeping(app: &App) {
    if app.view != View::Main {
        app.main.list.forget();
    }
}

/// Escape closes what is open.
pub fn on_key(app: &App, key: &KeyPress) -> Option<Message> {
    (app.view == View::Main && key.code == "Escape").then_some(Message::Main(Msg::Escape))
}

/// While playing, the playhead steps four times a second; a row marked from the session's line
/// loses its mark.
pub fn timers(app: &App) -> Vec<(Duration, Message)> {
    let mut t = Vec::new();
    // A recording started with the window hidden (by the hotkey) still stops the sound.
    if app.main.player.playing() && (!app.hidden || app.rec.busy()) {
        t.push((Duration::from_millis(250), Message::Main(Msg::PlayTick)));
    }
    if app.main.list.marked.is_some() {
        t.push((transcript::MARK_FOR, Message::Main(Msg::List(ListMsg::Unmark))));
    }
    t
}

fn select(app: &App) -> (&crate::model::transcript::Transcript, &ListState) {
    (&app.live, &app.main.list)
}

/// The title's box while it is renamed.
fn title_input(app: &App, ui: &mut Ui) -> Option<El> {
    let draft = app.main.list.title.as_ref()?;
    Some(card::title_box(
        ui,
        app.main.list.title_key.clone(),
        draft,
        |v| wrap_list(ListMsg::TitleInput(v)),
        wrap_list(ListMsg::TitleSave),
    ))
}

fn session_ctx(app: &App) -> SessionCtx<'_> {
    SessionCtx {
        app,
        tr: &app.live,
        state: &app.main.card,
        player: app.live.folder.is_some() && !app.live.audio.is_empty(),
        playing: app.main.player.playing(),
        playing_ms: app.main.playing_ms,
        on_new: true,
        back: false,
        wrap: wrap_card,
    }
}

/// The transcript between the card and the bar.
/// The transcript with the card floating over its top.
fn transcript_area(app: &App, ui: &mut Ui, ph: Phase, playable: bool, card: El) -> El {
    let top = transcript::card_height("main");
    let list = transcript::view(
        app,
        Ctx { select, live: ph == Phase::Live, editable: ph == Phase::Review, playable, wrap: wrap_list, top },
        ui,
    );
    transcript::under_card("main", card, list)
}

pub fn view(app: &App, ui: &mut Ui) -> El {
    let ph = phase(app);
    if app.compact() {
        return compact(app, ph, ui);
    }
    let banners = notices::banners(app);
    match ph {
        Phase::Start => {
            let top = div()
                .id(uid("start"))
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scrollbar()
                .child(div().px(g(2.0)).pt(g(1.0)).child(source_picker::picker(app)));
            col()
                .size_full()
                .child(banners)
                .child(top)
                .child(card::start_foot(app, source_picker::file_button(app)))
                .into_any_element()
        }
        Phase::Live => {
            let input = title_input(app, ui);
            let c = card::live_card(app, &app.live, input, wrap_list(ListMsg::RenameStart));
            // The card comes in from where 開始 was, at the bottom of the window.
            let from = app.main.enter_card.then_some(motion::From { dx: 0.0, dy: 60.0 * GRID });
            let look = Look { color: p().ink, side: p().ink_side, radius: R_PANEL };
            let c = motion::enter("live-card", c, from, false, look);
            let bar = if app.rec.state == UiState::Finishing {
                card::finishing_bar(app)
            } else {
                card::live_bar(app, wrap_card)
            };
            col().size_full().child(banners).child(transcript_area(app, ui, ph, false, c)).child(bar).into_any_element()
        }
        Phase::Review => {
            let input = title_input(app, ui);
            let c = session_ctx(app);
            let player = c.player;
            col()
                .size_full()
                .child(banners)
                .child(transcript_area(
                    app,
                    ui,
                    ph,
                    player,
                    card::session_card(c, input, wrap_list(ListMsg::RenameStart)),
                ))
                .child(card::review_bar(c))
                .into_any_element()
        }
    }
}

/// The compact window: what is recorded (dragging it moves the window), the latest line, and the
/// controls.
fn compact(app: &App, ph: Phase, ui: &mut Ui) -> El {
    let p_ = p();
    let menu = Some(Message::Titlebar(titlebar::Msg::SourceMenu));
    let (name, what): (String, String) = match ph {
        Phase::Start => ("Chidori".into(), String::new()),
        Phase::Live => (app.live.title.clone().unwrap_or_default(), sources_text(app.rec.sources.as_ref())),
        Phase::Review => (app.live.title.clone().unwrap_or_default(), String::new()),
    };
    let mut head = row()
        .flex_none()
        .h(g(5.0))
        .pl(g(2.0))
        .gap(g(1.0))
        .child(div().flex_shrink_1().min_w_0().overflow_hidden().child(line(name, tokens::LABEL, p_.ink).bold()));
    if !what.is_empty() {
        let ink = p_.ink;
        let switch = clickable(menu.clone(), "compact-source")
            .flex_shrink_1()
            .min_w_0()
            .overflow_hidden()
            .text_color(p_.muted)
            .hover(move |s| s.text_color(ink))
            .child(plain(what, tokens::LABEL).overflow_hidden().text_ellipsis());
        head = head.child(titlebar::solid(with_tip(switch, t("chooseSource")).into_any_element()));
    }
    let head = titlebar::with_window_menu(
        app,
        head.child(fill())
            .child(titlebar::compact_buttons())
            .id("compact-head")
            .window_control_area(WindowControlArea::Drag),
    );

    // The strip: the choice before a recording, the latest line after it starts.
    let strip_body: El = match ph {
        Phase::Start => {
            let ink = p_.ink;
            clickable(menu, "compact-choice")
                .flex()
                .items_center()
                .gap(g(1.0))
                .size_full()
                .text_color(ink)
                .child(div().flex_1().min_w_0().overflow_hidden().child(line(
                    source_picker::summary_text(app),
                    tokens::BODY,
                    ink,
                )))
                .child(icon(Icon::ChevronDown, 16.0, p_.muted))
                .into_any_element()
        }
        _ => transcript::tail(app, &app.live, &app.main.list),
    };
    let strip = row()
        .flex_none()
        .mx(g(1.0))
        .h(g(5.0))
        .px(g(2.0))
        .rounded(px(R_ROW))
        .bg(p_.surface)
        .overflow_hidden()
        .child(strip_body);

    let controls: El = match ph {
        Phase::Start => row()
            .w_full()
            .gap(g(1.0))
            .child(card::compact_start(app))
            .child(fill())
            .child(source_picker::file_button(app))
            .into_any_element(),
        Phase::Live if app.rec.state == UiState::Finishing => card::compact_finishing(app),
        Phase::Live => card::compact_live(app, &app.live, wrap_card),
        Phase::Review => card::compact_review(session_ctx(app)),
    };
    let _ = ui;
    col()
        .size_full()
        .bg(p_.ground)
        .child(notices::banners(app))
        .child(head)
        .child(strip)
        .child(row().flex_1().min_h_0().pl(g(2.0)).pr(g(1.0)).child(controls))
        .into_any_element()
}

#[cfg(test)]
#[path = "main/previews.rs"]
mod previews;
