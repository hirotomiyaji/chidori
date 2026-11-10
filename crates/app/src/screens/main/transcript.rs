//! The transcript: a list that builds only the rows in view (a long session stays as cheap as a
//! short one), following the newest line until the reader scrolls up (FR-40). Every line is a white
//! bubble, 相手 on the left and 自分 on the right; a turn's first line has the speaker's name in
//! their colour and the time above it. The words still coming are a hollow bubble in faint text. A
//! starred line has an ink ring and ★, as does the line being played. Screenshots are one-line
//! markers where they were taken, and markers say what happened. A finished session's lines can be
//! played from, starred, edited and deleted; a live one's starred. The compact window shows the
//! latest line instead.

use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use chidori_core::error::AppError;
use chidori_core::events::UiState;
use chidori_core::host::AppHandle;
use chidori_core::ops;
use chidori_core::session::model::{Marker, MarkerKind, Screenshot, Segment, SourceId, TimelineItem};
use gpui_kit::{
    FollowMode, InteractiveElement, IntoElement, ListAlignment, ListOffset, ParentElement, SharedString, Styled, div,
    list as gpui_list, px,
};

use super::rows::{self, Row, RowKind, Rows};
use crate::actions::Action;
use crate::app::{App, Message};
use crate::i18n::{t, tf};
use crate::icons::{Icon, icon};
use crate::model::format::{day_of, day_text};
use crate::model::transcript::Transcript;
use crate::motion::{self, Look};
use crate::screens::popup::Menu;
use crate::task::{self, Task};
use crate::theme::{self, Color, GRID, Ink, R_NOTCH, R_ROW, g, line, mix, p, plain, tokens};
use crate::ui::{self, El, Field, Ui, clickable};
use crate::widgets::{Size, Variant, btn, col, fill, floating, nothing, row, spinner, with_tip};

/// Speech this far behind is shown as being written. A fast GPU writes a line well within it, so
/// the note only appears where text is slow to come, such as on a CPU, which has no grey partials.
const WRITING_MS: u64 = 1_500;
/// The room the note takes under the last line.
const WRITING_H: f32 = 3.0 * GRID;
/// The widest the lines run, however wide the window.
const MAX_W: f32 = 760.0;
/// Rows built beyond the view, above and below.
const OVERSCAN: f32 = 360.0;
/// The side padding of a row.
const PAD_X: f32 = 2.0 * GRID;
/// The widest a bubble gets in the 420 px window; wider windows give it a like share.
const BUBBLE_W: f32 = 37.0 * GRID;

pub type Wrap = fn(ListMsg) -> Message;

/// A line being edited.
pub struct Edit {
    pub id: String,
    pub text: String,
}

/// The list's state: GPUI's list (which builds only the rows in view and measures them), the rows
/// it was last told about, and what is being edited.
pub struct ListState {
    pub items: gpui_kit::ListState,
    pub editor_key: SharedString,
    pub title_key: SharedString,
    pub caption_key: SharedString,
    /// The keys of the rows the list holds, and how they looked, so it is told what changed.
    keys: RefCell<Vec<String>>,
    look: Cell<u64>,
    /// The list's width as of the last layout, read before the rows are built (see `measure`).
    width: Cell<f32>,
    cache: RefCell<Option<(u64, Rc<Rows>)>>,
    pub editing: Option<Edit>,
    /// A screenshot's caption being written: its ID and the draft.
    pub caption: Option<(String, String)>,
    /// A row pointed at on the session's line, marked for a moment.
    pub marked: Option<(String, Instant)>,
    /// The line being played.
    pub playing: Option<String>,
    /// The title being renamed (the card's).
    pub title: Option<String>,
}

/// The key of the list's last item: the room under the rows (and the note while words are on
/// their way), which changes with the note.
fn footer_key(note: bool) -> String {
    format!("~footer-{note}")
}

impl ListState {
    pub fn new(name: &'static str) -> Self {
        let items = gpui_kit::ListState::new(0, ListAlignment::Top, px(OVERSCAN));
        items.set_scroll_handler(|_, window, _| window.refresh());
        Self {
            items,
            editor_key: format!("{name}-editor").into(),
            title_key: format!("{name}-title").into(),
            caption_key: format!("{name}-caption").into(),
            keys: RefCell::new(Vec::new()),
            look: Cell::new(0),
            width: Cell::new(0.0),
            cache: RefCell::new(None),
            editing: None,
            caption: None,
            marked: None,
            playing: None,
            title: None,
        }
    }

    /// A different session: its own rows and pictures.
    pub fn reset(&mut self) {
        self.editing = None;
        self.caption = None;
        self.marked = None;
        self.playing = None;
        self.title = None;
        self.keys.borrow_mut().clear();
        self.items.reset(0);
        self.forget();
    }

    /// The rows of a transcript, built again only when it changed.
    pub fn rows(&self, tr: &Transcript) -> Rc<Rows> {
        let sig = rows::signature(tr, false);
        let mut cache = self.cache.borrow_mut();
        if let Some((s, r)) = &*cache
            && *s == sig
        {
            return r.clone();
        }
        let r = Rc::new(rows::build(tr, false));
        *cache = Some((sig, r.clone()));
        r
    }

    /// Tells GPUI's list which rows there are now: new ones at the end are added, rows whose
    /// look may have changed are measured again.
    fn sync(&self, keys: Vec<String>, look: u64) {
        let mut old = self.keys.borrow_mut();
        if *old != keys {
            let same = old.iter().zip(&keys).take_while(|(a, b)| a == b).count();
            if old.is_empty() {
                self.items.reset(keys.len());
            } else {
                self.items.splice(same..old.len(), keys.len() - same);
                // The row before the change may have ended a turn or a partial.
                self.items.remeasure_items(same.saturating_sub(2)..same);
            }
            *old = keys;
        }
        if self.look.replace(look) != look {
            self.items.remeasure();
        }
    }

    /// The list is off screen: when it shows again it starts at the newest line.
    pub fn forget(&self) {
        self.items.set_follow_mode(FollowMode::Tail);
    }

    /// Stops following (a finished session opens at its first line).
    pub fn stay(&self) {
        self.items.set_follow_mode(FollowMode::Normal);
        self.items.scroll_to(ListOffset::default());
    }

    fn following(&self) -> bool {
        self.items.is_following_tail()
    }

    /// Brings a row into view.
    pub fn scroll_to_row(&self, index: usize) -> Task {
        self.items.set_follow_mode(FollowMode::Normal);
        self.items.scroll_to_reveal_item(index);
        Task::none()
    }

    /// Whether a row is in view.
    fn shows(&self, index: usize) -> bool {
        self.items.item_is_above_viewport(index) == Some(false)
            && self.items.item_is_below_viewport(index) == Some(false)
    }

    /// While playing, the transcript keeps up with the sound, unless the reader has scrolled away
    /// from the line that was playing.
    pub fn follow_playing(&mut self, tr: &Transcript, key: Option<String>) -> Task {
        let prev = std::mem::replace(&mut self.playing, key.clone());
        let Some(key) = key else { return Task::none() };
        if prev.as_deref() == Some(key.as_str()) {
            return Task::none();
        }
        let rows = self.rows(tr);
        let index_of = |k: &str| rows.rows.iter().position(|r| r.key == k);
        let Some(index) = index_of(&key) else { return Task::none() };
        let was_shown = prev.as_deref().and_then(index_of).is_none_or(|i| self.shows(i));
        if was_shown && !self.shows(index) { self.scroll_to_row(index) } else { Task::none() }
    }

    /// Notes the list's width for this frame. GPUI's list holds its state while it lays the rows
    /// out, and a row asking it for its width then panics ("already mutably borrowed"), so the
    /// width is read here, before the list is built, and rows read this copy.
    fn measure(&self) {
        self.width.set(f32::from(self.items.viewport_bounds().size.width));
    }

    fn width(&self) -> f32 {
        let w = self.width.get();
        if w > 0.0 { w.min(MAX_W) } else { crate::app::EXPANDED.width }
    }
}

#[derive(Debug, Clone)]
pub enum ListMsg {
    JumpLatest,
    PlayFrom(u64),
    Seek(u64),
    Unmark,
    Star(String, bool),
    Edit(String),
    EditInput(String),
    SaveEdit,
    CancelEdit,
    DeleteLine(String),
    Caption(String),
    CaptionInput(String),
    SaveCaption,
    DeleteShot(String),
    OpenImage(PathBuf),
    /// A line came back changed (edited or starred).
    Changed(Result<Segment, AppError>),
    /// An item was deleted, or its caption saved.
    Removed(String, Result<(), AppError>),
    CaptionSaved(String, String, Result<(), AppError>),
    RenameStart,
    TitleInput(String),
    TitleSave,
    TitleSaved(String, Result<(), AppError>),
    Escape,
}

/// Runs a blocking core call off the UI thread.
pub fn run<T: Send + 'static>(
    core: &AppHandle,
    f: impl FnOnce(&AppHandle) -> T + Send + 'static,
    map: impl FnOnce(T) -> Message + Send + 'static,
) -> Task {
    let core = core.clone();
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

fn failed(e: AppError) -> Message {
    Message::Failed(e, "saveFailed")
}

/// What a list's messages change: its state and its transcript.
pub fn update(core: &AppHandle, list: &mut ListState, tr: &mut Transcript, msg: ListMsg, wrap: Wrap) -> Task {
    let Some(sid) = tr.session_id.clone() else { return Task::none() };
    match msg {
        ListMsg::JumpLatest => {
            list.items.set_follow_mode(FollowMode::Tail);
            Task::none()
        }
        // Played by the screen, which owns the player.
        ListMsg::PlayFrom(_) => Task::none(),
        ListMsg::Seek(ms) => {
            let rows = list.rows(tr);
            let Some(index) = rows::seek_index(tr, &rows, ms) else { return Task::none() };
            list.marked = Some((rows.rows[index].key.clone(), Instant::now()));
            list.stay();
            list.scroll_to_row(index)
        }
        ListMsg::Unmark => {
            if list.marked.as_ref().is_some_and(|(_, at)| at.elapsed() >= MARK_FOR) {
                list.marked = None;
            }
            Task::none()
        }
        ListMsg::Star(id, important) => {
            run(core, move |c| ops::mark_segment(c, &sid, &id, important), move |r| wrap(ListMsg::Changed(r)))
        }
        ListMsg::Changed(Ok(seg)) => {
            if list.editing.as_ref().is_some_and(|e| e.id == seg.id) {
                list.editing = None;
            }
            tr.update_segment(seg);
            Task::none()
        }
        ListMsg::Changed(Err(e))
        | ListMsg::Removed(_, Err(e))
        | ListMsg::CaptionSaved(_, _, Err(e))
        | ListMsg::TitleSaved(_, Err(e)) => Task::done(failed(e)),
        ListMsg::Edit(id) => {
            let text = tr.items.iter().find_map(|i| match i {
                TimelineItem::Segment(s) if s.id == id => Some(s.text.clone()),
                _ => None,
            });
            let Some(text) = text else { return Task::none() };
            list.editing = Some(Edit { id, text });
            task::focus(list.editor_key.clone())
        }
        ListMsg::EditInput(v) => {
            if let Some(e) = &mut list.editing {
                e.text = v;
            }
            Task::none()
        }
        ListMsg::SaveEdit => {
            let Some(e) = &list.editing else { return Task::none() };
            let (id, text) = (e.id.clone(), e.text.clone());
            run(core, move |c| ops::update_segment(c, &sid, &id, &text), move |r| wrap(ListMsg::Changed(r)))
        }
        ListMsg::CancelEdit => {
            list.editing = None;
            Task::none()
        }
        ListMsg::DeleteLine(id) => run(
            core,
            move |c| ops::delete_segment(c, &sid, &id).map(|_| id),
            move |r| match r {
                Ok(id) => wrap(ListMsg::Removed(id, Ok(()))),
                Err(e) => wrap(ListMsg::Removed(String::new(), Err(e))),
            },
        ),
        ListMsg::DeleteShot(id) => run(
            core,
            move |c| ops::delete_screenshot(c, &sid, &id).map(|_| id),
            move |r| match r {
                Ok(id) => wrap(ListMsg::Removed(id, Ok(()))),
                Err(e) => wrap(ListMsg::Removed(String::new(), Err(e))),
            },
        ),
        ListMsg::Removed(id, Ok(())) => {
            tr.remove_item(&id);
            Task::none()
        }
        ListMsg::Caption(id) => {
            if list.caption.as_ref().is_some_and(|(c, _)| *c == id) {
                list.caption = None;
                return Task::none();
            }
            let current = tr.items.iter().find_map(|i| match i {
                TimelineItem::Screenshot(s) if s.id == id => s.caption.clone(),
                _ => None,
            });
            list.caption = Some((id, current.unwrap_or_default()));
            task::focus(list.caption_key.clone())
        }
        ListMsg::CaptionInput(v) => {
            if let Some((_, draft)) = &mut list.caption {
                *draft = v;
            }
            Task::none()
        }
        ListMsg::SaveCaption => {
            let Some((id, draft)) = list.caption.clone() else { return Task::none() };
            run(
                core,
                move |c| ops::set_caption(c, &sid, &id, &draft).map(|_| (id, draft)),
                move |r| match r {
                    Ok((id, draft)) => wrap(ListMsg::CaptionSaved(id, draft, Ok(()))),
                    Err(e) => wrap(ListMsg::CaptionSaved(String::new(), String::new(), Err(e))),
                },
            )
        }
        ListMsg::CaptionSaved(id, caption, Ok(())) => {
            tr.set_caption(&id, caption.trim());
            list.caption = None;
            Task::none()
        }
        ListMsg::OpenImage(path) => {
            if ops::allowed_path(core, &path) {
                let _ = open::that_detached(path);
            }
            Task::none()
        }
        ListMsg::RenameStart => {
            list.title = Some(tr.title.clone().unwrap_or_default());
            task::focus(list.title_key.clone())
        }
        ListMsg::TitleInput(v) => {
            list.title = Some(v);
            Task::none()
        }
        ListMsg::TitleSave => {
            let Some(draft) = list.title.take() else { return Task::none() };
            let next = draft.trim().to_string();
            if next.is_empty() || Some(&next) == tr.title.as_ref() {
                return Task::none();
            }
            run(
                core,
                move |c| ops::rename_session(c, &sid, &next).map(|_| next),
                move |r| match r {
                    Ok(title) => wrap(ListMsg::TitleSaved(title, Ok(()))),
                    Err(e) => wrap(ListMsg::TitleSaved(String::new(), Err(e))),
                },
            )
        }
        ListMsg::TitleSaved(title, Ok(())) => {
            tr.title = Some(title);
            Task::none()
        }
        ListMsg::Escape => {
            if list.editing.is_some() {
                list.editing = None;
            } else if list.caption.is_some() {
                list.caption = None;
            } else {
                list.title = None;
            }
            Task::none()
        }
    }
}

/// A row stays marked this long after the session's line pointed at it.
pub const MARK_FOR: Duration = Duration::from_millis(1600);

/// Speech heard but not written yet, from the core's lag (FR-25), while recording.
pub fn writing(app: &App) -> bool {
    matches!(app.rec.state, UiState::Recording | UiState::Paused)
        && !app.rec.deferred
        && app.rec.lag.as_ref().is_some_and(|l| l.lag_ms >= WRITING_MS)
}

/// 「文字起こし中…」 with a spinner, so a slow PC shows that the words are on their way.
pub fn writing_note(now: Instant) -> El {
    row()
        .gap(g(1.0))
        .child(spinner(16.0, now))
        .child(line(t("transcribing"), tokens::SMALL, p().muted))
        .into_any_element()
}

fn centered_note(content: El) -> gpui_kit::Div {
    div().size_full().flex().items_center().justify_center().px(g(3.0)).child(content)
}

/// Which list a view shows: the screen's transcript and list state, found again from the app when
/// GPUI builds a row.
pub type Select = fn(&App) -> (&Transcript, &ListState);

/// What a list shows and how.
#[derive(Clone, Copy)]
pub struct Ctx {
    pub select: Select,
    pub live: bool,
    pub editable: bool,
    /// Lines can be played from.
    pub playable: bool,
    pub wrap: Wrap,
}

/// The transcript as a list of the rows in view.
pub fn view(app: &App, c: Ctx, ui: &mut Ui) -> El {
    let (tr, list) = (c.select)(app);
    let wrap = c.wrap;
    // The boxes a row may hold are made here, where the window is at hand; the rows take them.
    if let Some(e) = &list.editing {
        ui.input(
            Field::new(list.editor_key.clone(), &e.text, move |v| wrap(ListMsg::EditInput(v)))
                .multi_line()
                .on_submit(wrap(ListMsg::SaveEdit)),
        );
    }
    if let Some((_, draft)) = &list.caption {
        ui.input(
            Field::new(list.caption_key.clone(), draft, move |v| wrap(ListMsg::CaptionInput(v)))
                .placeholder(t("caption"))
                .on_submit(wrap(ListMsg::SaveCaption)),
        );
    }
    let rows = list.rows(tr);
    let has_lines = rows.has_lines();
    let note = c.live && writing(app) && !rows.has_partial;
    if c.live && !has_lines {
        let body: El = if writing(app) {
            writing_note(app.now)
        } else {
            line(t("waitingForSpeech"), tokens::SMALL, p().muted).into_any_element()
        };
        return centered_note(body).into_any_element();
    }
    if !has_lines {
        return centered_note(line(t("emptyTranscript"), tokens::SMALL, p().muted).into_any_element())
            .into_any_element();
    }
    let mut keys: Vec<String> = rows.rows.iter().map(|r| r.key.clone()).collect();
    keys.push(footer_key(note));
    list.measure();
    list.sync(keys, look_key(app, tr, list, c));

    let items = gpui_list(list.items.clone(), move |ix, _window, cx| {
        ui::read_app(cx, |app| build_row(app, c, ix, note)).unwrap_or_else(nothing)
    })
    .size_full();
    let mut area = div().relative().size_full().child(items);
    if c.live && !list.following() {
        // The hover sits on the sheet itself: a wrapper's would be hidden under it.
        let hover = theme::hover_of(p().surface);
        let jump = floating(clickable(Some(wrap(ListMsg::JumpLatest)), "jump").flex().flex_row().items_center())
            .gap(g(1.0))
            .h(g(5.0))
            .px(g(2.0))
            .rounded(g(2.5))
            .hover(move |s| s.bg(hover))
            .child(icon(Icon::ArrowDown, 16.0, p().ink))
            .child(line(t("jumpLatest"), tokens::LABEL, p().ink).bold());
        area = area.child(div().absolute().left_0().right_0().bottom(g(2.0)).flex().justify_center().child(jump));
    }
    area.into_any_element()
}

/// A hash of everything a row's height depends on beyond its key, so the list measures again when
/// it changes.
fn look_key(app: &App, tr: &Transcript, list: &ListState, c: Ctx) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    rows::signature(tr, false).hash(&mut h);
    (c.live, c.editable, c.playable).hash(&mut h);
    crate::i18n::is_ja().hash(&mut h);
    list.caption.as_ref().map(|c| &c.0).hash(&mut h);
    list.editing.as_ref().map(|e| &e.id).hash(&mut h);
    (list.width() as u32).hash(&mut h);
    app.projects_version.hash(&mut h);
    h.finish()
}

/// One row of a list, built when GPUI lays it out.
fn build_row(app: &App, c: Ctx, ix: usize, note: bool) -> El {
    let (tr, list) = (c.select)(app);
    let rows = list.rows(tr);
    let Some(r) = rows.rows.get(ix) else {
        // The footer: room under the last line, and the note.
        let mut f = col().w_full().pb(g(2.0));
        if note {
            f = f.child(div().pl(px(PAD_X)).pt(g(1.0)).h(px(WRITING_H)).child(writing_note(app.now)));
        }
        return f.into_any_element();
    };
    let ringed =
        list.marked.as_ref().is_some_and(|(k, _)| *k == r.key) || list.playing.as_deref() == Some(r.key.as_str());
    let body: El = match r.kind {
        RowKind::Header => nothing(),
        RowKind::Item(i) => match &tr.items[i] {
            TimelineItem::Segment(s) if list.editing.as_ref().is_some_and(|e| e.id == s.id) => {
                edit_row(tr, s, r.head, rows.two, list, c.wrap)
            }
            _ => row_view(app, tr, list, r, rows.two, ringed, c),
        },
        RowKind::Partial(_) => {
            row_view(app, tr, list, r, rows.two, false, Ctx { editable: false, playable: false, ..c })
        }
    };
    let inner = div().w_full().max_w(px(MAX_W)).child(body);
    div().w_full().flex().justify_center().child(inner).into_any_element()
}

/// A speaker's name in their colour: 相手 coral, 自分 teal.
pub fn speaker_color(source: SourceId) -> Color {
    if source == SourceId::Mic { p().me } else { p().them }
}

pub fn speaker_name(source: SourceId) -> String {
    t(if source == SourceId::Mic { "me" } else { "others" })
}

/// Lines of 自分 sit on the right when both sides were recorded.
fn on_right(source: SourceId, two: bool) -> bool {
    two && source == SourceId::Mic
}

/// The line over a bubble: the speaker's name (with both sides recorded) and the time, and ★ on a
/// starred line. 自分's reads the other way round, in line with its bubble on the right.
fn meta(source: SourceId, time: String, two: bool, name: bool, star: bool) -> El {
    let right = on_right(source, two);
    let who = (two && name).then(|| line(speaker_name(source), tokens::META, speaker_color(source)).bold());
    let when = line(time, tokens::META, p().meta);
    let mut r = row().h(g(3.0)).gap(px(4.0));
    if right {
        if star {
            r = r.child(line("★ ·", tokens::META, p().ink).bold());
        }
        r = r.child(when);
        if let Some(w) = who {
            r = r.child(w);
        }
    } else {
        if let Some(w) = who {
            r = r.child(w);
        }
        r = r.child(when);
        if star {
            r = r.child(line("· ★", tokens::META, p().ink).bold());
        }
    }
    r.into_any_element()
}

/// A small action in a row's hover box.
fn act(which: Icon, label: String, msg: Message, pressed: bool) -> El {
    let group = format!("act-{which:?}");
    let ink = p().ink;
    let hover = theme::wash();
    let mut glyph = icon(which, 16.0, if pressed { ink } else { p().muted });
    if !pressed {
        glyph = glyph.group_hover(group.clone(), move |s| s.text_color(ink));
    }
    let b = clickable(Some(msg), "act")
        .group(group)
        .flex()
        .items_center()
        .justify_center()
        .size(g(4.0))
        .rounded(g(2.0))
        .hover(move |s| s.bg(hover))
        .child(glyph);
    with_tip(b, label).into_any_element()
}

/// A row with actions shown at its top, on the side away from its bubble, while the pointer is on it.
/// `menu` is what a right click on the row opens: the same actions, as a menu.
fn with_actions(body: El, items: Vec<El>, key: &str, right: bool, menu: Message) -> El {
    let group: SharedString = format!("row-{key}").into();
    let bar = floating(row()).p(px(2.0)).children(items);
    let overlay = div().absolute().top_0().invisible().group_hover(group.clone(), |s| s.visible()).child(bar);
    let overlay = if right { overlay.left(px(PAD_X)) } else { overlay.right(px(PAD_X)) };
    let row = div().id(ui::uid(("row", key))).group(group).relative().w_full().child(body).child(overlay);
    ui::on_right(row, menu).into_any_element()
}

fn row_view(app: &App, tr: &Transcript, list: &ListState, r: &Row, two: bool, ringed: bool, c: Ctx) -> El {
    match r.kind {
        RowKind::Header => nothing(),
        RowKind::Partial(source) => {
            let p_ = tr.partials.get(&source);
            let text = p_.map_or_else(String::new, |p| p.text.clone());
            let time = p_.and_then(|p| p.t_start_ms).map(|t| tr.clock(t)).unwrap_or_default();
            let b =
                bubble(theme::t(text, tokens::BODY, p().faint).into_any_element(), source, two, list, Bubble::Hollow);
            line_row(r.head.then(|| meta(source, time, two, true, false)), b, source, two)
        }
        RowKind::Item(i) => match &tr.items[i] {
            TimelineItem::Segment(s) => segment_row(app, tr, list, s, r.head, two, ringed, c),
            TimelineItem::Screenshot(s) => screenshot_row(tr, list, s, c.editable && !c.live, c.wrap),
            TimelineItem::Marker(m) => marker_row(tr, m),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bubble {
    /// White.
    Plain,
    /// White with the ink ring: starred, or being played.
    Ringed,
    /// The words still coming: no fill, a `line` ring.
    Hollow,
}

/// How wide a bubble may get: about three quarters of the list.
fn bubble_w(list: &ListState) -> f32 {
    ((list.width() - 2.0 * PAD_X) * 0.77).max(BUBBLE_W.min(list.width() - 2.0 * PAD_X))
}

/// A bubble around a line's words: 16 round, 4 at the top corner on the speaker's side.
fn bubble(content: El, source: SourceId, two: bool, list: &ListState, kind: Bubble) -> El {
    let right = on_right(source, two);
    let p_ = p();
    let ring = kind != Bubble::Plain;
    // The ring draws inside the box, so the words keep their place.
    let (py, px_) = if ring { (GRID - 2.0, 2.0 * GRID - 2.0) } else { (GRID, 2.0 * GRID) };
    let mut b = div().max_w(px(bubble_w(list))).py(px(py)).px(px(px_)).rounded(px(R_ROW));
    b = if right { b.rounded_tr(px(R_NOTCH)) } else { b.rounded_tl(px(R_NOTCH)) };
    b = match kind {
        Bubble::Plain => b.bg(p_.surface),
        Bubble::Ringed => b.bg(p_.surface).border_2().border_color(p_.ink),
        Bubble::Hollow => b.border_2().border_color(p_.line),
    };
    b.child(content).into_any_element()
}

/// A line's place in the list: its meta over its bubble, on its speaker's side.
fn line_row(meta: Option<El>, bubble: El, source: SourceId, two: bool) -> El {
    let right = on_right(source, two);
    let mut c = col().w_full().px(px(PAD_X)).pt(g(1.0));
    c = if right { c.items_end() } else { c.items_start() };
    if let Some(m) = meta {
        c = c.child(m);
    }
    c.child(bubble).into_any_element()
}

/// A line's words, and 編集済み under them when edited.
fn line_text(s: &Segment, color: Color) -> El {
    let body = theme::t(s.text.clone(), tokens::BODY, color);
    if !s.edited {
        return body.into_any_element();
    }
    col().child(body).child(line(t("edited"), tokens::SMALL, p().muted)).into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn segment_row(
    app: &App,
    tr: &Transcript,
    list: &ListState,
    s: &Segment,
    head: bool,
    two: bool,
    ringed: bool,
    c: Ctx,
) -> El {
    let fresh = tr.fresh.contains_key(&s.id);
    // A fresh line darkens into place.
    let ink = match tr.fresh.get(&s.id) {
        Some(at) => {
            let t = (app.now.saturating_duration_since(*at).as_secs_f32() / 0.2).clamp(0.0, 1.0);
            mix(p().ink, p().faint, t)
        }
        None => p().ink,
    };
    let kind = if s.important || ringed { Bubble::Ringed } else { Bubble::Plain };
    let b = bubble(line_text(s, ink), s.source, two, list, kind);
    // A new line comes in from below into its place.
    let look = Look { color: p().surface, side: p().side, radius: R_ROW };
    let from = fresh.then_some(motion::From { dx: 0.0, dy: 2.0 * GRID });
    let b = motion::enter(format!("line-{}", s.id), b, from, false, look);
    let meta = (head || s.important).then(|| meta(s.source, tr.clock(s.t_start_ms), two, head, s.important));
    let body = line_row(meta, b, s.source, two);
    if tr.session_id.is_none() {
        return body;
    }
    let wrap = c.wrap;
    let mut items = Vec::new();
    let mut menu = Menu::new().act(t("copyLine"), Some(Message::Action(Action::CopyText(s.text.clone()))));
    if c.playable {
        let play = wrap(ListMsg::PlayFrom(s.t_start_ms));
        items.push(act(Icon::Play, t("playFromHere"), play.clone(), false));
        menu = menu.act(t("playFromHere"), Some(play));
    }
    let mark = if s.important { t("unmarkImportant") } else { t("markImportant") };
    let star = wrap(ListMsg::Star(s.id.clone(), !s.important));
    items.push(act(Icon::Star, mark.clone(), star.clone(), s.important));
    menu = menu.act(mark, Some(star));
    if c.editable && !c.live {
        let (edit, delete) = (wrap(ListMsg::Edit(s.id.clone())), wrap(ListMsg::DeleteLine(s.id.clone())));
        items.push(act(Icon::Pencil, t("editLine"), edit.clone(), false));
        items.push(act(Icon::Trash, t("deleteLine"), delete.clone(), false));
        menu = menu.separator().act(t("editLine"), Some(edit)).act(t("deleteLine"), Some(delete));
    }
    with_actions(body, items, &s.id, on_right(s.source, two), menu.open())
}

/// A line being edited: a white panel with its words in a box, and 取消 and 保存 under it.
fn edit_row(tr: &Transcript, s: &Segment, head: bool, two: bool, list: &ListState, wrap: Wrap) -> El {
    let editor = ui::built_input(&list.editor_key).unwrap_or_else(nothing);
    let buttons = row()
        .gap(g(1.0))
        .child(fill())
        .child(btn(t("cancel"), Variant::Ghost, Size::Sm, Some(wrap(ListMsg::CancelEdit))))
        .child(btn(t("save"), Variant::Primary, Size::Sm, Some(wrap(ListMsg::SaveEdit))));
    let panel = col().w_full().gap(g(1.0)).p(g(1.0)).rounded(px(R_ROW)).bg(p().surface).child(editor).child(buttons);
    let meta = head.then(|| meta(s.source, tr.clock(s.t_start_ms), two, true, s.important));
    let mut c = col().w_full().px(px(PAD_X)).pt(g(1.0));
    if let Some(m) = meta {
        c = c.child(m);
    }
    c.child(panel).into_any_element()
}

/// A screenshot: one line in the middle of the flow, where it was taken; clicking opens it.
fn screenshot_row(tr: &Transcript, list: &ListState, s: &Screenshot, editable: bool, wrap: Wrap) -> El {
    let path = tr.folder.as_ref().map(|f| crate::app::session_file(f, &s.file));
    let mut words = format!("{} {}", tr.clock(s.t_ms), t("screenshotShort"));
    let open = list.caption.as_ref().filter(|(id, _)| *id == s.id);
    if let (Some(caption), None) = (&s.caption, open) {
        words = format!("{words} · {caption}");
    }
    let ink = p().ink;
    let content = row()
        .gap(g(1.0))
        .h(g(3.0))
        .min_w_0()
        .child(icon(Icon::Camera, 16.0, p().muted).group_hover("shot", move |st| st.text_color(ink)))
        .child(div().min_w_0().overflow_hidden().child(plain(words, tokens::SMALL).text_ellipsis()));
    let open_image = path.map(|p| wrap(ListMsg::OpenImage(p)));
    let mut menu = Menu::new().act(t("openImage"), open_image.clone());
    if editable {
        menu = menu
            .separator()
            .act(t("addCaption"), Some(wrap(ListMsg::Caption(s.id.clone()))))
            .act(t("deleteScreenshot"), Some(wrap(ListMsg::DeleteShot(s.id.clone()))));
    }
    let menu = menu.open();
    let marker = clickable(open_image, ("shot", &s.id))
        .group("shot")
        .flex()
        .min_w_0()
        .text_color(p().muted)
        .hover(move |st| st.text_color(ink))
        .child(content);
    let marker = with_tip(marker, t("openImage"));
    let mut c = col().w_full().items_center().px(px(PAD_X)).pt(g(1.0)).child(marker);
    if open.is_some() {
        let input = ui::built_input(&list.caption_key).unwrap_or_else(nothing);
        let save = btn(t("save"), Variant::Primary, Size::Sm, Some(wrap(ListMsg::SaveCaption)));
        c = c.child(row().w_full().gap(g(1.0)).pt(g(1.0)).child(div().flex_1().child(input)).child(save));
    }
    if !editable {
        let row = div().id(ui::uid(("shot-row", &s.id))).w_full().child(c);
        return ui::on_right(row, menu).into_any_element();
    }
    with_actions(
        c.into_any_element(),
        vec![
            act(Icon::ImagePlus, t("addCaption"), wrap(ListMsg::Caption(s.id.clone())), false),
            act(Icon::Trash, t("deleteScreenshot"), wrap(ListMsg::DeleteShot(s.id.clone())), false),
        ],
        &s.id,
        false,
        menu,
    )
}

/// A marker, small in the middle of the flow with what happened. A cut starts a part, so it is in
/// ink and stands further from the part before.
fn marker_row(tr: &Transcript, m: &Marker) -> El {
    let detail = m.detail.clone().unwrap_or_else(|| "?".into());
    let (glyph, label) = match m.kind {
        MarkerKind::SourceReattached => (Icon::Link, t("markerReattached")),
        MarkerKind::Paused => (Icon::Pause, t("markerPaused")),
        MarkerKind::Resumed => (Icon::Play, t("markerResumed")),
        MarkerKind::Cut => (Icon::Scissors, t("markerCut")),
        MarkerKind::SourceChanged => (Icon::ArrowRightLeft, tf("markerSourceChanged", &[("source", &detail)])),
        MarkerKind::Continued => (Icon::StepForward, t("markerContinued")),
        MarkerKind::Unprocessed => (Icon::TriangleAlert, tf("markerUnprocessed", &[("n", &detail)])),
    };
    // A continuation on another day says which (FR-08).
    let day = match (&m.detail, &tr.started_at) {
        (Some(d), Some(start)) if m.kind == MarkerKind::Continued && d.as_str() != day_of(start) => {
            format!("{} ", day_text(d, chrono::Datelike::year(&chrono::Local::now())))
        }
        _ => String::new(),
    };
    let cut = m.kind == MarkerKind::Cut;
    let tone = if cut { p().ink } else { p().muted };
    let text = line(label, tokens::SMALL, tone);
    let mut label_row = row().flex_none().h(g(3.0)).gap(g(1.0)).child(icon(glyph, 16.0, tone)).child(if cut {
        text.bold()
    } else {
        text
    });
    if m.kind != MarkerKind::Unprocessed {
        label_row = label_row.child(line(format!("{day}{}", tr.clock(m.t_ms)), tokens::SMALL, tone));
    }
    let rule = || div().flex_1().h(px(1.0)).bg(p().line);
    let mut r = row().w_full().gap(g(2.0)).px(px(PAD_X)).pt(g(if cut { 3.0 } else { 1.0 }));
    if cut {
        r = r.child(rule()).child(label_row).child(rule());
    } else {
        r = r.justify_center().child(label_row);
    }
    r.into_any_element()
}

/// The compact window's strip: the latest line, cut at its start so the newest words show.
pub fn tail(app: &App, tr: &Transcript, list: &ListState) -> El {
    let rows = list.rows(tr);
    let shown = rows::tail(tr, &rows);
    let Some(last) = shown.last() else {
        let body = if writing(app) {
            writing_note(app.now)
        } else {
            line(t("waitingForSpeech"), tokens::SMALL, p().muted).into_any_element()
        };
        return body;
    };
    // The compact window's width is fixed; what fits in the strip is about this many ems.
    let avail = crate::app::COMPACT.width - 2.0 * GRID - 4.0 * GRID;
    let label = |source: SourceId| -> Option<El> {
        rows.two.then(|| line(speaker_name(source), tokens::META, speaker_color(source)).bold().into_any_element())
    };
    match last.kind {
        RowKind::Partial(s) => {
            let text = tr.partials.get(&s).map_or("", |p| p.text.as_str());
            tail_line(label(s), text, p().faint, avail)
        }
        RowKind::Item(i) => match &tr.items[i] {
            TimelineItem::Screenshot(s) => row()
                .gap(g(1.0))
                .child(icon(Icon::Camera, 16.0, p().muted))
                .child(line(format!("{} {}", tr.clock(s.t_ms), t("screenshotShort")), tokens::SMALL, p().muted))
                .into_any_element(),
            TimelineItem::Segment(s) => tail_line(label(s.source), &s.text, p().ink, avail),
            TimelineItem::Marker(_) => nothing(),
        },
        RowKind::Header => nothing(),
    }
}

/// One line of the tail, cut at its start so the newest words stay visible.
fn tail_line(label: Option<El>, text: &str, color: Color, avail: f32) -> El {
    let mut r = row().w_full().gap(g(1.0));
    let mut room = avail;
    if let Some(l) = label {
        r = r.child(div().flex_none().child(l));
        room -= tokens::META.size * 2.0 + GRID;
    }
    let size = tokens::BODY.size;
    let fits = |s: &str| s.chars().map(|c| if c.is_ascii() { size * 0.55 } else { size }).sum::<f32>() <= room;
    let shown = if fits(text) {
        text.to_string()
    } else {
        let chars: Vec<char> = text.chars().collect();
        let mut from = 0;
        while from < chars.len() && !fits(&format!("…{}", chars[from..].iter().collect::<String>())) {
            from += 1;
        }
        format!("…{}", chars[from..].iter().collect::<String>())
    };
    r.child(div().flex_1().min_w_0().overflow_hidden().child(line(shown, tokens::BODY, color))).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_before_any_layout_is_the_expanded_window() {
        let list = ListState::new("t");
        list.measure();
        assert_eq!(list.width(), crate::app::EXPANDED.width);
    }

    /// Rows read the copy `measure` took, never GPUI's list, which is borrowed while it lays them out.
    #[test]
    fn width_comes_from_the_measured_copy() {
        let list = ListState::new("t");
        list.width.set(500.0);
        assert_eq!(list.width(), 500.0);
        list.width.set(2_000.0);
        assert_eq!(list.width(), MAX_W);
    }
}
