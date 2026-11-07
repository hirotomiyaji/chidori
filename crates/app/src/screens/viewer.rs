//! A past session opened from 履歴: read, play, edit lines, rename, copy, export (FR-41, FR-62). It
//! has its own transcript, so a recording that runs meanwhile keeps its own.

use std::time::Duration;

use chidori_core::error::AppError;
use chidori_core::ops::{self, SessionView};
use gpui_kit::{IntoElement, ParentElement, Styled, div, px};

use super::main_view::deck::{self, DeckAnim, DeckCtx, DeckMsg, DeckState};
use super::main_view::playback::{Player, Status};
use super::main_view::transcript::{self, Ctx, LINE_Y, ListMsg, ListState};
use super::main_view::{headers, rows};
use crate::app::{App, Level, Message, View, call};
use crate::i18n::t;
use crate::model::transcript::Transcript;
use crate::task::Task;
use crate::theme::{line, p, tokens};
use crate::ui::{El, KeyPress, Ui};
use crate::widgets::{col, spinner};

pub struct State {
    pub transcript: Transcript,
    pub list: ListState,
    pub deck: DeckAnim,
    pub player: Player,
    pub playing_ms: Option<u64>,
    /// The session asked for could not be read.
    pub failed: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            transcript: Transcript::default(),
            list: ListState::new("viewer"),
            deck: DeckAnim::default(),
            player: Player::default(),
            playing_ms: None,
            failed: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Loaded(String, Result<Box<SessionView>, AppError>),
    List(ListMsg),
    Deck(DeckMsg),
    PlayTick,
    Escape,
}

fn wrap_list(m: ListMsg) -> Message {
    Message::Viewer(Msg::List(m))
}

fn wrap_deck(m: DeckMsg) -> Message {
    Message::Viewer(Msg::Deck(m))
}

pub fn update(app: &mut App, msg: Msg) -> Task {
    match msg {
        Msg::Loaded(id, r) => {
            if app.viewing_session.as_deref() != Some(id.as_str()) {
                return Task::none();
            }
            match r {
                Ok(v) => {
                    app.viewer.transcript.load(*v);
                    Task::none()
                }
                Err(e) => {
                    app.viewer.failed = true;
                    Task::done(Message::Failed(e, "failed"))
                }
            }
        }
        Msg::List(ListMsg::PlayFrom(ms)) => {
            play_from(app, ms);
            Task::none()
        }
        Msg::List(m) => {
            let core = app.core.clone();
            let v = &mut app.viewer;
            transcript::update(&core, &mut v.list, &mut v.transcript, m, wrap_list, true)
        }
        Msg::Deck(DeckMsg::Seek(ms)) => update(app, Msg::List(ListMsg::Seek(ms))),
        Msg::Deck(DeckMsg::Play) => {
            match app.viewer.player.status() {
                Status::Playing => app.viewer.player.pause(),
                Status::Paused => app.viewer.player.resume(),
                _ => play_from(app, 0),
            }
            Task::none()
        }
        Msg::Deck(DeckMsg::Export(id)) => {
            super::dialogs::ask_export(app, id);
            Task::none()
        }
        // A past session has no camera.
        Msg::Deck(DeckMsg::ShotMenu | DeckMsg::ShotWindows(_)) => Task::none(),
        Msg::PlayTick => {
            if app.view != View::Session {
                stop(app);
                return Task::none();
            }
            let v = &mut app.viewer;
            let status = v.player.status();
            if matches!(status, Status::Ended | Status::Failed) {
                v.player.reset();
            }
            v.playing_ms = v.player.position();
            let rows = v.list.rows(&v.transcript, true);
            let key = v.playing_ms.and_then(|ms| rows::playing_key(&v.transcript, &rows, ms));
            let task = v.list.follow_playing(&v.transcript, true, key);
            if status == Status::Failed {
                app.toast(t("audioMissing"), Level::Warn);
            }
            task
        }
        Msg::Escape => update(app, Msg::List(ListMsg::Escape)),
    }
}

fn play_from(app: &mut App, ms: u64) {
    let v = &mut app.viewer;
    let Some(folder) = &v.transcript.folder else { return };
    let files: Vec<_> =
        v.transcript.audio.iter().map(|a| (crate::app::session_file(folder, &a.file), a.start_ms)).collect();
    if files.is_empty() {
        return;
    }
    v.player.play_from(files, ms);
    v.playing_ms = Some(ms);
}

fn stop(app: &mut App) {
    app.viewer.player.stop();
    app.viewer.playing_ms = None;
    app.viewer.list.playing = None;
}

pub fn on_menu(_app: &mut App, _rest: &str) -> Task {
    Task::none()
}

pub fn open(app: &mut App, id: String) -> Task {
    stop(app);
    let v = &mut app.viewer;
    v.transcript.clear();
    v.list.reset();
    v.list.stay();
    v.failed = false;
    v.deck.forget();
    call(
        app,
        {
            let id = id.clone();
            move |c| ops::get_session(c, &id).map(Box::new)
        },
        move |r| Message::Viewer(Msg::Loaded(id, r)),
    )
}

pub fn tick(app: &mut App) -> Task {
    if app.view != View::Session {
        app.viewer.deck.forget();
    }
    Task::none()
}

pub fn animating(app: &App) -> bool {
    app.view == View::Session && app.viewer.deck.moving(app.now)
}

/// Keeps the deck fresh for when the view shows again.
pub fn housekeeping(app: &App) {
    if app.view != View::Session {
        app.viewer.deck.forget();
    }
}

/// Escape closes what is open.
pub fn on_key(app: &App, key: &KeyPress) -> Option<Message> {
    (app.view == View::Session && key.code == "Escape").then_some(Message::Viewer(Msg::Escape))
}

/// While playing, the playhead steps four times a second.
pub fn timers(app: &App) -> Vec<(Duration, Message)> {
    let mut t = Vec::new();
    if app.view != View::Session {
        return t;
    }
    if app.viewer.player.playing() && !app.hidden {
        t.push((Duration::from_millis(250), Message::Viewer(Msg::PlayTick)));
    }
    if app.viewer.list.marked.is_some() {
        t.push((transcript::MARK_FOR, Message::Viewer(Msg::List(ListMsg::Unmark))));
    }
    t
}

fn select(app: &App) -> (&Transcript, &ListState) {
    (&app.viewer.transcript, &app.viewer.list)
}

fn header(app: &App) -> El {
    headers::session_header(app, &app.viewer.transcript, &app.viewer.list, wrap_list)
}

pub fn view(app: &App, ui: &mut Ui) -> El {
    let v = &app.viewer;
    let tr = &v.transcript;
    if tr.session_id.is_none() {
        let body: El = if v.failed {
            line(t("loadFailed"), tokens::BODY_SMALL, p().ink_muted).into_any_element()
        } else {
            spinner(20.0, app.now)
        };
        return div().size_full().flex().items_center().justify_center().child(body).into_any_element();
    }
    v.deck.observe(DeckState::Review, 0);
    let player = tr.folder.is_some() && !tr.audio.is_empty();
    let ctx = DeckCtx {
        app,
        tr,
        anim: &v.deck,
        compact: false,
        state: DeckState::Review,
        player,
        playing: v.player.playing(),
        playing_ms: v.playing_ms,
        on_new: false,
        wrap: wrap_deck,
    };
    let deck_el = deck::view(ctx);
    let list = transcript::view(
        app,
        Ctx {
            select,
            live: false,
            editable: true,
            header: Some(header as fn(&App) -> El),
            playable: player,
            wrap: wrap_list,
            under: LINE_Y,
        },
        ui,
    );
    div()
        .relative()
        .size_full()
        .child(div().absolute().top_0().left_0().right_0().bottom(px(deck::EXPANDED.height - LINE_Y)).child(list))
        .child(col().absolute().inset_0().justify_end().child(deck_el))
        .into_any_element()
}
