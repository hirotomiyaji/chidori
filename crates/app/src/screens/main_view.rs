//! 録音: what to listen to and 開始 before a recording, the recording with its line, and the
//! session that just finished, each above the same deck, which stays from the start dot to the
//! finished session so its line can move between them (DECISIONS → UI). The compact window keeps
//! only the choice, or the last two lines, above its deck.

#[path = "main/app_wave.rs"]
mod app_wave;
#[path = "main/deck.rs"]
pub mod deck;
#[path = "main/headers.rs"]
pub mod headers;
#[path = "main/live_line.rs"]
mod live_line;
#[path = "main/playback.rs"]
pub mod playback;
#[path = "main/popover.rs"]
mod popover;
#[path = "main/rows.rs"]
pub mod rows;
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
use gpui_kit::{IntoElement, ParentElement, Styled, div, px};

use self::deck::{DeckAnim, DeckCtx, DeckMsg, DeckState};
use self::playback::{Player, Status};
use self::source_picker::{PickMsg, PickerState};
use self::transcript::{Ctx, LINE_Y, ListMsg, ListState};
use crate::app::{App, Level, Message, View, call};
use crate::i18n::t;
use crate::model::source::ChosenApp;
use crate::screens::notices;
use crate::task::Task;
use crate::theme::{Ink, line, p, tokens};
use crate::ui::{El, KeyPress, Ui};
use crate::widgets::col;

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

fn deck_state(app: &App, phase: Phase) -> DeckState {
    match phase {
        Phase::Start => DeckState::Start,
        Phase::Review => DeckState::Review,
        Phase::Live => match app.rec.state {
            UiState::Paused => DeckState::Paused,
            UiState::Finishing => DeckState::Finishing,
            _ => DeckState::Recording,
        },
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
    pub picker: PickerState,
    pub list: ListState,
    pub deck: DeckAnim,
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
}

impl Default for State {
    fn default() -> Self {
        Self {
            picker: PickerState::default(),
            list: ListState::new("main"),
            deck: DeckAnim::default(),
            player: Player::default(),
            playing_ms: None,
            windows: Vec::new(),
            live_session: None,
            applied: None,
            list_session: None,
            last_phase: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Pick(PickMsg),
    List(ListMsg),
    Deck(DeckMsg),
    /// Several messages from one press.
    Batch(Vec<Message>),
    /// The settings at the start of a recording, which may have come from the hotkey.
    LiveSettings(Box<Settings>),
    Switched(Result<(), AppError>, Box<Choice>),
    PlayTick,
    Escape,
}

fn wrap_list(m: ListMsg) -> Message {
    Message::Main(Msg::List(m))
}

fn wrap_deck(m: DeckMsg) -> Message {
    Message::Main(Msg::Deck(m))
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
            let ph = phase(app);
            transcript::update(&core, &mut app.main.list, &mut app.live, m, wrap_list, ph == Phase::Review)
        }
        Msg::Deck(DeckMsg::Seek(ms)) => update(app, Msg::List(ListMsg::Seek(ms))),
        Msg::Deck(DeckMsg::Play) => {
            toggle_play(app);
            Task::none()
        }
        Msg::Deck(DeckMsg::Export(id)) => {
            crate::screens::dialogs::ask_export(app, id);
            Task::none()
        }
        Msg::Deck(DeckMsg::ShotMenu) => call(
            app,
            |c| ops::list_windows(c).unwrap_or_default(),
            |w| Message::Main(Msg::Deck(DeckMsg::ShotWindows(w))),
        ),
        Msg::Deck(DeckMsg::ShotWindows(windows)) => {
            let items = deck::shot_menu(app, windows.clone());
            app.main.windows = windows;
            Task::done(Message::Popup(items, None))
        }
        Msg::Batch(msgs) => Task::batch(msgs.into_iter().map(Task::done)),
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
        Msg::Escape => {
            if app.main.picker.open != source_picker::Open::None {
                app.main.picker.open = source_picker::Open::None;
                Task::none()
            } else if app.main.picker.panel {
                app.main.picker.panel = false;
                Task::none()
            } else {
                update(app, Msg::List(ListMsg::Escape))
            }
        }
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

/// The apps whose sound is on screen.
fn sync_watch(app: &mut App) -> Task {
    let ph = phase(app);
    let shown = app.view == View::Main && !app.hidden && (ph == Phase::Start || (ph == Phase::Live && !app.compact()));
    let pids = if shown { source_picker::shown_pids(app) } else { Vec::new() };
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
    let rows = app.main.list.rows(&app.live, true);
    let key = app.main.playing_ms.and_then(|ms| rows::playing_key(&app.live, &rows, ms));
    app.main.list.follow_playing(&app.live, true, key)
}

pub fn tick(app: &mut App) -> Task {
    let mut tasks = vec![sync_watch(app)];
    if app.view != View::Main {
        app.main.deck.forget();
    }
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
    // A new session on screen starts its list afresh; a finished one opens at its header.
    if app.main.list_session != app.live.session_id {
        app.main.list_session = app.live.session_id.clone();
        app.main.list.reset();
    }
    let ph = phase(app);
    if app.view == View::Main && app.main.last_phase != Some(ph) {
        let was = app.main.last_phase.replace(ph);
        match ph {
            Phase::Review if was.is_some() => app.main.list.stay(),
            Phase::Live => app.main.list.forget(),
            _ => {}
        }
        if ph != Phase::Review {
            app.main.player.stop();
            app.main.playing_ms = None;
        }
    }
    Task::batch(tasks)
}

/// The line moves: while recording (the pen), and while the deck moves between states.
pub fn animating(app: &App) -> bool {
    if app.view != View::Main || app.needs_wizard() {
        return false;
    }
    let state = deck_state(app, phase(app));
    app.rec.state == UiState::Recording || app.main.deck.moving(app.now) || pending(app, state)
}

/// A change of state the deck hasn't drawn yet, which may start a move.
fn pending(app: &App, state: DeckState) -> bool {
    let drawn = app.main.deck.drawn_state();
    drawn.is_some_and(|d| d != state)
}

pub fn spinning(app: &App) -> bool {
    if app.view != View::Main {
        return false;
    }
    let ph = phase(app);
    app.rec.state == UiState::Finishing
        || (ph == Phase::Live && transcript::writing(app))
        || (app.source.loading_apps && (app.main.picker.open != source_picker::Open::None || app.main.picker.panel))
        || (ph == Phase::Start
            && app.rec.engine.as_ref().is_some_and(|e| e.state == chidori_core::asr::worker::EngineState::Loading))
}

/// Keeps the list and the deck fresh for when the view shows again.
pub fn housekeeping(app: &App) {
    if app.view != View::Main {
        app.main.deck.forget();
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
    if app.main.player.playing() && !app.hidden {
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

fn header(app: &App) -> El {
    headers::session_header(app, &app.live, &app.main.list, wrap_list)
}

pub fn view(app: &App, ui: &mut Ui) -> El {
    let ph = phase(app);
    let compact = app.compact();
    let state = deck_state(app, ph);
    app.main.deck.observe(state, app.line.samples.len());
    let player = ph == Phase::Review && app.live.folder.is_some() && !app.live.audio.is_empty();
    let ctx = DeckCtx {
        app,
        tr: &app.live,
        anim: &app.main.deck,
        compact,
        state,
        player,
        playing: app.main.player.playing(),
        playing_ms: app.main.playing_ms,
        on_new: true,
        wrap: wrap_deck,
    };
    let deck_h = ctx.height();
    let deck_el = deck::view(ctx);
    let banners = notices::banners(app);

    if compact {
        let body: El = if ph == Phase::Start {
            let open = app.main.picker.panel;
            div()
                .size_full()
                .flex()
                .items_center()
                .px(px(12.0))
                .child(source_picker::summary(app, open, 40.0, Message::Main(Msg::Pick(PickMsg::Panel(!open)))))
                .into_any_element()
        } else {
            col()
                .size_full()
                .justify_end()
                .overflow_hidden()
                .px(px(12.0))
                .pt(px(2.0))
                .pb(px(4.0))
                .child(transcript::tail(app, &app.live, &app.main.list))
                .into_any_element()
        };
        let mut page =
            col().relative().size_full().child(banners).child(div().flex_1().min_h_0().child(body)).child(deck_el);
        if ph == Phase::Start && app.main.picker.panel {
            // Over the whole window under the title bar, the start row included.
            page = page.child(source_picker::panel(app));
        }
        return page.into_any_element();
    }

    if ph == Phase::Start {
        let label = line(t("listenTo"), tokens::CAPTION, p().ink_muted).medium().px(px(2.0));
        let start =
            div().w_full().flex().justify_center().px(px(16.0)).pt(px(12.0)).pb(px(8.0)).child(
                col().w_full().max_w(px(380.0)).gap(px(8.0)).child(label).child(source_picker::picker(app, false)),
            );
        let mut c = col().size_full().child(banners).child(start);
        if deck_h.is_some() {
            c = c.child(div().flex_1());
        }
        return c.child(deck_el).into_any_element();
    }

    let deck_h = deck_h.unwrap_or(deck::EXPANDED.height);
    let list = transcript::view(
        app,
        Ctx {
            select,
            live: ph == Phase::Live,
            editable: ph == Phase::Review,
            header: (ph == Phase::Review).then_some(header as fn(&App) -> El),
            playable: player,
            wrap: wrap_list,
            under: LINE_Y,
        },
        ui,
    );
    // The transcript runs on under the deck down to its line.
    let area = div()
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .child(div().absolute().top_0().left_0().right_0().bottom(px(deck_h - LINE_Y)).child(list))
        .child(col().absolute().inset_0().justify_end().child(deck_el));
    let mut c = col().size_full().child(banners);
    if ph == Phase::Live && state != DeckState::Finishing {
        c = c.child(headers::live_header(app, &app.live, &app.main.list, wrap_list));
    }
    c.child(area).into_any_element()
}

#[cfg(test)]
#[path = "main/previews.rs"]
mod previews;
