//! A past session opened from 履歴: read, play, edit lines, rename, copy, export (FR-41, FR-62). It
//! has its own transcript, so a recording that runs meanwhile keeps its own. It looks like the
//! session that just finished in 録音, with a way back to 履歴 on its card.

use std::time::Duration;

use chidori_core::error::AppError;
use chidori_core::ops::{self, SessionView};
use gpui_kit::{IntoElement, ParentElement, Styled, div};

use super::main_view::card::{self, CardMsg, CardState, SessionCtx};
use super::main_view::playback::{Player, Status};
use super::main_view::rows;
use super::main_view::transcript::{self, Ctx, ListMsg, ListState};
use crate::app::{App, Level, Message, View, call};
use crate::i18n::t;
use crate::model::transcript::Transcript;
use crate::task::Task;
use crate::theme::{g, line, p, tokens};
use crate::ui::{El, KeyPress, Ui};
use crate::widgets::{col, spinner};

pub struct State {
    pub transcript: Transcript,
    pub list: ListState,
    pub card: CardState,
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
            card: CardState::default(),
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
    Card(CardMsg),
    PlayTick,
    Escape,
}

fn wrap_list(m: ListMsg) -> Message {
    Message::Viewer(Msg::List(m))
}

fn wrap_card(m: CardMsg) -> Message {
    Message::Viewer(Msg::Card(m))
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
            transcript::update(&core, &mut v.list, &mut v.transcript, m, wrap_list)
        }
        Msg::Card(CardMsg::Seek(ms)) => {
            if app.viewer.player.status() != Status::Idle {
                play_from(app, ms);
            }
            update(app, Msg::List(ListMsg::Seek(ms)))
        }
        Msg::Card(CardMsg::Play) => {
            match app.viewer.player.status() {
                Status::Playing => app.viewer.player.pause(),
                Status::Paused => app.viewer.player.resume(),
                _ => play_from(app, 0),
            }
            Task::none()
        }
        Msg::Card(CardMsg::Export(id)) => {
            super::dialogs::ask_export(app, id);
            Task::none()
        }
        // A past session has no camera.
        Msg::Card(CardMsg::ShotMenu | CardMsg::ShotWindows(_)) => Task::none(),
        Msg::PlayTick => {
            // Leaving the session stops its sound, and so does a recording starting, which would
            // otherwise take it in through the speakers.
            if app.view != View::Session || app.rec.busy() {
                stop(app);
                return Task::none();
            }
            let v = &mut app.viewer;
            let status = v.player.status();
            if matches!(status, Status::Ended | Status::Failed) {
                v.player.reset();
            }
            v.playing_ms = v.player.position();
            let rows = v.list.rows(&v.transcript);
            let key = v.playing_ms.and_then(|ms| rows::playing_key(&v.transcript, &rows, ms));
            let task = v.list.follow_playing(&v.transcript, key);
            if status == Status::Failed {
                app.toast(t("audioMissing"), Level::Warn);
            }
            task
        }
        Msg::Escape => update(app, Msg::List(ListMsg::Escape)),
    }
}

fn play_from(app: &mut App, ms: u64) {
    if app.rec.busy() {
        return;
    }
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
    call(
        app,
        {
            let id = id.clone();
            move |c| ops::get_session(c, &id).map(Box::new)
        },
        move |r| Message::Viewer(Msg::Loaded(id, r)),
    )
}

pub fn tick(_app: &mut App) -> Task {
    Task::none()
}

/// Escape closes what is open.
pub fn on_key(app: &App, key: &KeyPress) -> Option<Message> {
    (app.view == View::Session && key.code == "Escape").then_some(Message::Viewer(Msg::Escape))
}

/// While playing, the playhead steps four times a second.
pub fn timers(app: &App) -> Vec<(Duration, Message)> {
    let mut t = Vec::new();
    // Ticks on after the view is left, or once a recording starts with the window hidden, so the
    // tick can stop the sound.
    if app.viewer.player.playing() && (!app.hidden || app.view != View::Session || app.rec.busy()) {
        t.push((Duration::from_millis(250), Message::Viewer(Msg::PlayTick)));
    }
    if app.view != View::Session {
        return t;
    }
    if app.viewer.list.marked.is_some() {
        t.push((transcript::MARK_FOR, Message::Viewer(Msg::List(ListMsg::Unmark))));
    }
    t
}

fn select(app: &App) -> (&Transcript, &ListState) {
    (&app.viewer.transcript, &app.viewer.list)
}

pub fn view(app: &App, ui: &mut Ui) -> El {
    let v = &app.viewer;
    let tr = &v.transcript;
    if tr.session_id.is_none() {
        let body: El = if v.failed {
            line(t("loadFailed"), tokens::BODY, p().muted).into_any_element()
        } else {
            spinner(24.0, app.now)
        };
        return div().size_full().flex().items_center().justify_center().child(body).into_any_element();
    }
    let player = tr.folder.is_some() && !tr.audio.is_empty();
    let c = SessionCtx {
        app,
        tr,
        state: &v.card,
        player,
        playing: v.player.playing(),
        playing_ms: v.playing_ms,
        on_new: false,
        back: true,
        wrap: wrap_card,
    };
    let input = v.list.title.as_ref().map(|draft| {
        card::title_box(
            ui,
            v.list.title_key.clone(),
            draft,
            |s| wrap_list(ListMsg::TitleInput(s)),
            wrap_list(ListMsg::TitleSave),
        )
    });
    let list =
        transcript::view(app, Ctx { select, live: false, editable: true, playable: player, wrap: wrap_list }, ui);
    col()
        .size_full()
        .child(div().flex_none().px(g(2.0)).child(card::session_card(c, input, wrap_list(ListMsg::RenameStart))))
        .child(div().flex_1().min_h_0().w_full().child(list))
        .child(card::review_bar(c))
        .into_any_element()
}
