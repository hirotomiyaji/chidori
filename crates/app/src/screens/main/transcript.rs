//! The transcript: a list that builds only the rows in view (a long session stays as cheap as a
//! short one), following the newest line until the reader scrolls up (FR-40). Turns name the
//! speaker only when both sides were recorded; the words still coming are pencil grey and darken
//! into place; screenshots sit where they were taken; markers are rules with what happened. A
//! finished session's lines can be played from, starred, edited and deleted; a live one's starred.
//! The compact window shows the last two lines instead.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
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
    img, list as gpui_list, px,
};

use super::rows::{self, Row, RowKind, Rows};
use crate::app::{App, Message};
use crate::i18n::{t, tf};
use crate::icons::{Icon, icon};
use crate::model::format::{day_of, day_text};
use crate::model::transcript::Transcript;
use crate::paint::Picture;
use crate::task::{self, Task};
use crate::theme::{self, Color, Ink, line, mix, p, tokens};
use crate::ui::{self, El, Field, Ui, clickable};
use crate::widgets::{Size, Variant, btn_with, col, fill, floating, frame, nothing, row, spinner, with_tip};

/// How far below the expanded deck's top its line lies; the transcript runs on under the deck down
/// to it, so only the tint under the line covers the text.
pub const LINE_Y: f32 = 30.0;
/// Speech this far behind is shown as being written. A fast GPU writes a line well within it, so
/// the note only appears where text is slow to come, such as on a CPU, which has no grey partials.
const WRITING_MS: u64 = 1_500;
/// The room the note takes under the last line.
const WRITING_H: f32 = 28.0;
/// The widest the lines run, however wide the window.
const MAX_W: f32 = 760.0;
/// Rows built beyond the view, above and below.
const OVERSCAN: f32 = 360.0;
/// The side padding of a row.
const PAD_X: f32 = 18.0;

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
    cache: RefCell<Option<(u64, Rc<Rows>)>>,
    pub editing: Option<Edit>,
    /// A screenshot's caption being written: its ID and the draft.
    pub caption: Option<(String, String)>,
    /// A row pointed at on the session's line, marked for a moment.
    pub marked: Option<(String, Instant)>,
    /// The line being played.
    pub playing: Option<String>,
    thumbs: HashMap<String, Picture>,
    decoding: HashSet<String>,
    /// The title being renamed (the header's or the recording's).
    pub title: Option<String>,
    /// 詳細 is open under the finished session's header.
    pub details: bool,
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
            cache: RefCell::new(None),
            editing: None,
            caption: None,
            marked: None,
            playing: None,
            thumbs: HashMap::new(),
            decoding: HashSet::new(),
            title: None,
            details: false,
        }
    }

    /// A different session: its own rows and pictures.
    pub fn reset(&mut self) {
        self.thumbs.clear();
        self.decoding.clear();
        self.editing = None;
        self.caption = None;
        self.marked = None;
        self.playing = None;
        self.title = None;
        self.details = false;
        self.keys.borrow_mut().clear();
        self.items.reset(0);
        self.forget();
    }

    /// The rows of a transcript, built again only when it changed.
    pub fn rows(&self, tr: &Transcript, header: bool) -> Rc<Rows> {
        let sig = rows::signature(tr, header);
        let mut cache = self.cache.borrow_mut();
        if let Some((s, r)) = &*cache
            && *s == sig
        {
            return r.clone();
        }
        let r = Rc::new(rows::build(tr, header));
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

    /// Stops following (a finished session opens at its header).
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
    pub fn follow_playing(&mut self, tr: &Transcript, header: bool, key: Option<String>) -> Task {
        let prev = std::mem::replace(&mut self.playing, key.clone());
        let Some(key) = key else { return Task::none() };
        if prev.as_deref() == Some(key.as_str()) {
            return Task::none();
        }
        let rows = self.rows(tr, header);
        let index_of = |k: &str| rows.rows.iter().position(|r| r.key == k);
        let Some(index) = index_of(&key) else { return Task::none() };
        let was_shown = prev.as_deref().and_then(index_of).is_none_or(|i| self.shows(i));
        if was_shown && !self.shows(index) { self.scroll_to_row(index) } else { Task::none() }
    }

    fn width(&self) -> f32 {
        let w = f32::from(self.items.viewport_bounds().size.width);
        if w > 0.0 { w.min(MAX_W) } else { crate::app::EXPANDED.width }
    }
}

/// A screenshot's size in the list: at most 220 px high and the width of the lines.
fn image_size(s: &Screenshot, max_w: f32) -> (f32, f32) {
    let (w, h) = (s.width.max(1) as f32, s.height.max(1) as f32);
    let height = 220.0_f32.min(max_w * h / w);
    (height * w / h, height)
}

#[derive(Debug, Clone)]
pub enum ListMsg {
    /// A row came into view.
    Shown(String),
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
    Thumb(String, Option<Picture>),
    /// A line came back changed (edited or starred).
    Changed(Result<Segment, AppError>),
    /// An item was deleted, or its caption saved.
    Removed(String, Result<(), AppError>),
    CaptionSaved(String, String, Result<(), AppError>),
    RenameStart,
    TitleInput(String),
    TitleSave,
    TitleSaved(String, Result<(), AppError>),
    Details,
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

/// What a list's messages change: its state and its transcript. `header` says whether the rows
/// start with the session's header.
pub fn update(
    core: &AppHandle,
    list: &mut ListState,
    tr: &mut Transcript,
    msg: ListMsg,
    wrap: Wrap,
    header: bool,
) -> Task {
    let Some(sid) = tr.session_id.clone() else { return Task::none() };
    match msg {
        ListMsg::Shown(key) => {
            // A saved screenshot is read from its file once it comes into view.
            let shot = tr.items.iter().find_map(|i| match i {
                TimelineItem::Screenshot(s) if s.id == key => Some(s.clone()),
                _ => None,
            });
            match (shot, &tr.folder) {
                (Some(s), Some(folder))
                    if !tr.thumbs.contains_key(&s.id)
                        && !list.thumbs.contains_key(&s.id)
                        && list.decoding.insert(s.id.clone()) =>
                {
                    let path = crate::app::session_file(folder, &s.file);
                    let id = s.id.clone();
                    Task::future(async move {
                        let handle = chidori_core::host::runtime()
                            .spawn_blocking(move || decode_thumb(&path))
                            .await
                            .ok()
                            .flatten();
                        wrap(ListMsg::Thumb(id, handle))
                    })
                }
                _ => Task::none(),
            }
        }
        ListMsg::Thumb(id, handle) => {
            list.decoding.remove(&id);
            if let Some(h) = handle {
                list.thumbs.insert(id, h);
            }
            Task::none()
        }
        ListMsg::JumpLatest => {
            list.items.set_follow_mode(FollowMode::Tail);
            Task::none()
        }
        // Played by the screen, which owns the player.
        ListMsg::PlayFrom(_) => Task::none(),
        ListMsg::Seek(ms) => {
            let rows = list.rows(tr, header);
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
        ListMsg::Details => {
            list.details = !list.details;
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

/// A saved screenshot, read and shrunk to what the list shows (2× for sharp screens).
fn decode_thumb(path: &std::path::Path) -> Option<Picture> {
    let img = ::image::open(path).ok()?.thumbnail(1520, 440).into_rgba8();
    let (w, h) = img.dimensions();
    Picture::from_rgba(w, h, img.into_raw())
}

/// Speech heard but not written yet, from the core's lag (FR-25), while recording.
pub fn writing(app: &App) -> bool {
    matches!(app.rec.state, UiState::Recording | UiState::Paused)
        && !app.rec.deferred
        && app.rec.lag.as_ref().is_some_and(|l| l.lag_ms >= WRITING_MS)
}

/// 「文字起こし中…」 with a spinner, so a slow PC shows that the words are on their way.
pub fn writing_note(now: Instant) -> El {
    row()
        .gap(px(6.0))
        .child(spinner(12.0, now))
        .child(line(t("transcribing"), tokens::CAPTION, p().ink_muted))
        .into_any_element()
}

fn centered_note(content: El) -> gpui_kit::Div {
    div().size_full().flex().items_center().justify_center().px(px(24.0)).child(content)
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
    /// The finished session's header, as the first row.
    pub header: Option<fn(&App) -> El>,
    /// Lines can be played from.
    pub playable: bool,
    pub wrap: Wrap,
    /// How far the deck reaches over the bottom of the list.
    pub under: f32,
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
    if let Some(draft) = &list.title {
        ui.input(
            Field::new(list.title_key.clone(), draft, move |v| wrap(ListMsg::TitleInput(v)))
                .placeholder(t("newTitle"))
                .on_submit(wrap(ListMsg::TitleSave)),
        );
    }
    let rows = list.rows(tr, c.header.is_some());
    let has_lines = rows.has_lines();
    let note = c.live && writing(app) && !rows.has_partial;
    if c.live && !has_lines {
        let body: El = if writing(app) {
            writing_note(app.now)
        } else {
            line(t("waitingForSpeech"), tokens::CAPTION, p().ink_muted).into_any_element()
        };
        return centered_note(body).pb(px(c.under)).into_any_element();
    }
    if !has_lines && c.header.is_none() {
        return centered_note(line(t("emptyTranscript"), tokens::CAPTION, p().ink_muted).into_any_element())
            .into_any_element();
    }
    let mut keys: Vec<String> = rows.rows.iter().map(|r| r.key.clone()).collect();
    keys.push(footer_key(note));
    list.sync(keys, look_key(app, tr, list, c));

    let items = gpui_list(list.items.clone(), move |ix, _window, cx| {
        let built = ui::read_app(cx, |app| build_row(app, c, ix, note));
        match built {
            Some((el, shown)) => {
                if let Some(key) = shown {
                    ui::send(wrap(ListMsg::Shown(key)), cx);
                }
                el
            }
            None => nothing(),
        }
    })
    .size_full();
    let mut area = div().relative().size_full().child(items);
    if c.live && !list.following() {
        let sunk = p().surface_sunken;
        let jump = floating(row())
            .gap(px(6.0))
            .px(px(14.0))
            .py(px(6.0))
            .rounded(px(tokens::RADIUS_MD))
            .child(icon(Icon::ArrowDown, 13.0, p().ink))
            .child(line(t("jumpLatest"), tokens::CAPTION, p().ink).medium());
        let jump = clickable(Some(wrap(ListMsg::JumpLatest)), "jump").hover(move |s| s.bg(sunk)).child(jump);
        area = area
            .child(div().absolute().left_0().right_0().bottom(px(c.under + 16.0)).flex().justify_center().child(jump));
    }
    area.into_any_element()
}

/// A hash of everything a row's height depends on beyond its key, so the list measures again when
/// it changes.
fn look_key(app: &App, tr: &Transcript, list: &ListState, c: Ctx) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    rows::signature(tr, c.header.is_some()).hash(&mut h);
    (c.live, c.editable, c.playable).hash(&mut h);
    crate::i18n::is_ja().hash(&mut h);
    (list.thumbs.len(), tr.thumbs.len()).hash(&mut h);
    list.caption.as_ref().map(|c| &c.0).hash(&mut h);
    list.editing.as_ref().map(|e| &e.id).hash(&mut h);
    (list.title.is_some(), list.details).hash(&mut h);
    app.projects_version.hash(&mut h);
    h.finish()
}

/// One row of a list, built when GPUI lays it out, and the key of a row that came into view and
/// wants something loaded.
fn build_row(app: &App, c: Ctx, ix: usize, note: bool) -> (El, Option<String>) {
    let (tr, list) = (c.select)(app);
    let rows = list.rows(tr, c.header.is_some());
    let Some(r) = rows.rows.get(ix) else {
        // The footer: room under the last line for the deck, and the note.
        let mut f = col().w_full().pb(px(20.0 + c.under));
        if note {
            f = f.child(div().pl(px(PAD_X)).pt(px(8.0)).h(px(WRITING_H)).child(writing_note(app.now)));
        }
        return (f.into_any_element(), None);
    };
    let mut shown = None;
    let body: El = match r.kind {
        RowKind::Header => {
            let mut col = col().w_full().child(c.header.map_or_else(nothing, |h| h(app)));
            if !rows.has_lines() {
                col = col.child(div().w_full().flex().justify_center().py(px(40.0)).child(line(
                    t("emptyTranscript"),
                    tokens::CAPTION,
                    p().ink_muted,
                )));
            }
            col.into_any_element()
        }
        RowKind::Item(i) => match &tr.items[i] {
            TimelineItem::Segment(s) if list.editing.as_ref().is_some_and(|e| e.id == s.id) => {
                edit_row(tr, s, r.head, rows.two, list, c.wrap)
            }
            TimelineItem::Screenshot(s) => {
                if !tr.thumbs.contains_key(&s.id) && !list.thumbs.contains_key(&s.id) && !list.decoding.contains(&s.id)
                {
                    shown = Some(s.id.clone());
                }
                row_view(app, tr, list, r, rows.two, c.editable && !c.live, c.playable, c.wrap)
            }
            _ => row_view(app, tr, list, r, rows.two, c.editable && !c.live, c.playable, c.wrap),
        },
        RowKind::Partial(_) => row_view(app, tr, list, r, rows.two, false, false, c.wrap),
    };
    let highlighted =
        list.marked.as_ref().is_some_and(|(k, _)| *k == r.key) || list.playing.as_deref() == Some(r.key.as_str());
    let mut inner = div().w_full().max_w(px(MAX_W)).child(body);
    if highlighted {
        inner = inner.bg(p().surface_sunken);
    }
    (div().w_full().flex().justify_center().child(inner).into_any_element(), shown)
}

/// How far a fresh line has darkened into place, 0..1; `None` once it has settled.
fn fresh_age(app: &App, tr: &Transcript, key: &str) -> Option<f32> {
    let at = tr.fresh.get(key)?;
    let t = app.now.saturating_duration_since(*at).as_secs_f32() / 0.2;
    (t < 1.0).then_some(t.max(0.0))
}

/// A speaker's name in the colour of their side of the line: 相手 in the accent, 自分 ink.
pub fn speaker_color(source: SourceId) -> Color {
    if source == SourceId::Mic { p().ink } else { p().accent }
}

pub fn speaker_name(source: SourceId) -> String {
    t(if source == SourceId::Mic { "me" } else { "others" })
}

/// A turn's first line: the speaker's name (with both sides recorded), then the time.
fn turn_head(source: SourceId, time: String, two: bool) -> El {
    let mut r = row().gap(px(8.0));
    if two {
        r = r.child(line(speaker_name(source), tokens::CAPTION, speaker_color(source)).medium());
    }
    r.child(line(time, tokens::CAPTION, p().ink_muted).mono()).into_any_element()
}

/// A small action in a row's hover box.
fn act(which: Icon, label: String, msg: Message, pressed: bool) -> El {
    let group = format!("act-{which:?}");
    let ink = p().ink;
    let sunk = p().surface_sunken;
    let mut glyph = icon(which, 13.0, if pressed { ink } else { p().ink_muted });
    if !pressed {
        glyph = glyph.group_hover(group.clone(), move |s| s.text_color(ink));
    }
    let b = clickable(Some(msg), "act")
        .group(group)
        .flex()
        .items_center()
        .justify_center()
        .size(px(24.0))
        .rounded(px(tokens::RADIUS_SM))
        .hover(move |s| s.bg(sunk))
        .child(glyph);
    with_tip(b, label).into_any_element()
}

/// A row with actions shown over its top right while the pointer is on it.
fn with_actions(body: El, items: Vec<El>, key: &str) -> El {
    let group: SharedString = format!("row-{key}").into();
    let bar = row()
        .gap(px(2.0))
        .p(px(2.0))
        .bg(p().surface)
        .border_1()
        .border_color(p().border)
        .rounded(px(tokens::RADIUS_MD))
        .children(items);
    let overlay =
        div().absolute().top(px(4.0)).right(px(8.0)).invisible().group_hover(group.clone(), |s| s.visible()).child(bar);
    div().id(ui::uid(("row", key))).group(group).relative().w_full().child(body).child(overlay).into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn row_view(
    app: &App,
    tr: &Transcript,
    list: &ListState,
    r: &Row,
    two: bool,
    editable: bool,
    playable: bool,
    wrap: Wrap,
) -> El {
    match r.kind {
        RowKind::Header => nothing(),
        RowKind::Partial(source) => {
            let p_ = tr.partials.get(&source);
            let text = p_.map_or_else(String::new, |p| p.text.clone());
            let time = p_.and_then(|p| p.t_start_ms).map(|t| tr.clock(t)).unwrap_or_default();
            let mut c = col().w_full().px(px(PAD_X)).pt(px(if r.head { 14.0 } else { 2.0 })).pb(px(2.0));
            if r.head {
                c = c.child(turn_head(source, time, two)).child(div().h(px(2.0)));
            }
            c.child(theme::t(text, tokens::BODY, p().ink_subtle)).into_any_element()
        }
        RowKind::Item(i) => match &tr.items[i] {
            TimelineItem::Segment(s) => segment_row(app, tr, s, r.head, two, editable, playable, wrap),
            TimelineItem::Screenshot(s) => screenshot_row(tr, list, s, editable, wrap),
            TimelineItem::Marker(m) => marker_row(tr, m),
        },
    }
}

/// A line's text: bolder when important, with 編集済み after it when edited.
fn line_text(s: &Segment, color: Color) -> El {
    let body = theme::t(s.text.clone(), tokens::BODY, color);
    let body = if s.important { body.medium() } else { body };
    if !s.edited {
        return body.into_any_element();
    }
    let tag = div()
        .flex_none()
        .px(px(4.0))
        .border_1()
        .border_color(p().border)
        .rounded(px(3.0))
        .text_size(px(tokens::CAPTION.size - 2.0))
        .text_color(p().ink_muted)
        .child(t("edited"));
    div().flex().flex_wrap().items_end().gap(px(4.0)).child(body).child(tag).into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn segment_row(
    app: &App,
    tr: &Transcript,
    s: &Segment,
    head: bool,
    two: bool,
    editable: bool,
    playable: bool,
    wrap: Wrap,
) -> El {
    let ink = match fresh_age(app, tr, &s.id) {
        Some(age) => mix(p().ink, p().ink_subtle, age),
        None => p().ink,
    };
    let mut c = col().relative().w_full().px(px(PAD_X)).pt(px(if head { 14.0 } else { 2.0 })).pb(px(2.0));
    if head {
        c = c.child(turn_head(s.source, tr.clock(s.t_start_ms), two)).child(div().h(px(2.0)));
    }
    c = c.child(line_text(s, ink));
    // An important line: an ink stroke in the margin, 11 px left of the text, and bolder text.
    if s.important {
        let top = if head { 14.0 + tokens::CAPTION.line_height + 2.0 } else { 2.0 };
        c = c.child(
            div()
                .absolute()
                .left(px(PAD_X - 11.0))
                .top(px(top + 6.0))
                .bottom(px(8.0))
                .w(px(3.0))
                .rounded(px(1.5))
                .bg(p().ink),
        );
    }
    let body = c.into_any_element();
    if tr.session_id.is_none() {
        return body;
    }
    let mut items = Vec::new();
    if playable {
        items.push(act(Icon::Play, t("playFromHere"), wrap(ListMsg::PlayFrom(s.t_start_ms)), false));
    }
    let mark = if s.important { t("unmarkImportant") } else { t("markImportant") };
    items.push(act(Icon::Star, mark, wrap(ListMsg::Star(s.id.clone(), !s.important)), s.important));
    if editable {
        items.push(act(Icon::Pencil, t("editLine"), wrap(ListMsg::Edit(s.id.clone())), false));
        items.push(act(Icon::Trash, t("deleteLine"), wrap(ListMsg::DeleteLine(s.id.clone())), false));
    }
    with_actions(body, items, &s.id)
}

/// A line being edited, in the kamon's frame: the one container being worked in.
fn edit_row(tr: &Transcript, s: &Segment, head: bool, two: bool, list: &ListState, wrap: Wrap) -> El {
    let mut c = col().w_full().px(px(PAD_X)).pt(px(if head { 14.0 } else { 2.0 })).pb(px(2.0));
    if head {
        c = c.child(turn_head(s.source, tr.clock(s.t_start_ms), two)).child(div().h(px(2.0)));
    }
    let editor = ui::built_input(&list.editor_key).unwrap_or_else(nothing);
    let buttons = row()
        .gap(px(6.0))
        .child(fill())
        .child(btn_with(
            row().gap(px(6.0)).child(icon(Icon::X, 13.0, p().ink)).child(theme::plain(t("cancel"), tokens::LABEL)),
            Variant::Ghost,
            Size::Sm,
            Some(wrap(ListMsg::CancelEdit)),
        ))
        .child(btn_with(
            row()
                .gap(px(6.0))
                .child(icon(Icon::Check, 13.0, p().on_accent))
                .child(theme::plain(t("save"), tokens::LABEL)),
            Variant::Primary,
            Size::Sm,
            Some(wrap(ListMsg::SaveEdit)),
        ));
    c.child(
        col()
            .gap(px(6.0))
            .py(px(4.0))
            .child(frame(div().p(px(2.0)).child(editor), p().surface_raised, tokens::RADIUS_LG))
            .child(buttons),
    )
    .into_any_element()
}

fn screenshot_row(tr: &Transcript, list: &ListState, s: &Screenshot, editable: bool, wrap: Wrap) -> El {
    let time = tr.clock(s.t_ms);
    let head = row()
        .gap(px(6.0))
        .child(icon(Icon::Camera, 12.0, p().ink_muted))
        .child(line(time, tokens::CAPTION, p().ink_muted).mono());
    let path = tr.folder.as_ref().map(|f| crate::app::session_file(f, &s.file));
    let handle = tr.thumbs.get(&s.id).or_else(|| list.thumbs.get(&s.id)).cloned();
    let (w, h) = image_size(s, (list.width() - 2.0 * PAD_X).max(40.0));
    let picture: El = match handle {
        Some(handle) => {
            let strong = p().border_strong;
            let b = clickable(path.map(|p| wrap(ListMsg::OpenImage(p))), ("shot", &s.id))
                .flex_none()
                .p(px(1.0))
                .bg(p().surface_sunken)
                .border_1()
                .border_color(p().border)
                .rounded(px(tokens::RADIUS_SM))
                .hover(move |st| st.border_color(strong))
                .child(img(handle.0).w(px(w)).h(px(h)));
            with_tip(b, t("openImage")).into_any_element()
        }
        // Not read yet, or no file: an empty frame of a fixed size.
        None => div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(192.0))
            .h(px(80.0))
            .border_1()
            .border_color(p().border)
            .rounded(px(tokens::RADIUS_SM))
            .child(icon(Icon::Camera, 18.0, p().ink_muted))
            .into_any_element(),
    };
    let mut c =
        col().w_full().px(px(PAD_X)).pt(px(14.0)).pb(px(4.0)).child(head).child(div().h(px(4.0))).child(picture);
    let open = list.caption.as_ref().filter(|(id, _)| *id == s.id);
    if let (Some(caption), None) = (&s.caption, open) {
        c = c.child(theme::t(caption.clone(), tokens::CAPTION, p().ink_muted).pt(px(6.0)));
    }
    if open.is_some() {
        let input = ui::built_input(&list.caption_key).unwrap_or_else(nothing);
        let save = btn_with(
            icon(Icon::Check, 13.0, p().on_accent),
            Variant::Primary,
            Size::Sm,
            Some(wrap(ListMsg::SaveCaption)),
        );
        c = c.child(row().gap(px(6.0)).pt(px(6.0)).child(div().flex_1().child(input)).child(save));
    }
    let body = c.into_any_element();
    if !editable {
        return body;
    }
    with_actions(
        body,
        vec![
            act(Icon::ImagePlus, t("addCaption"), wrap(ListMsg::Caption(s.id.clone())), false),
            act(Icon::Trash, t("deleteScreenshot"), wrap(ListMsg::DeleteShot(s.id.clone())), false),
        ],
        &s.id,
    )
}

/// A marker as a rule with what happened. A pause or a gap had no sound, so its line stays flat; a
/// cut starts a part, so its line is darker and it stands further from the part before.
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
    let tone = if cut { p().ink } else { p().ink_muted };
    let mut label_row = row()
        .flex_none()
        .gap(px(6.0))
        .child(icon(glyph, 11.0, tone))
        .child(line(label, tokens::CAPTION, tone).medium());
    if m.kind != MarkerKind::Unprocessed {
        label_row = label_row.child(line(format!("{day}{}", tr.clock(m.t_ms)), tokens::CAPTION, tone).mono());
    }
    let rule = || div().flex_1().h(px(1.0)).bg(if cut { p().border_strong } else { p().border });
    let (top, bottom) = if cut { (24.0, 8.0) } else { (12.0, 12.0) };
    row()
        .w_full()
        .gap(px(12.0))
        .px(px(PAD_X))
        .pt(px(top))
        .pb(px(bottom))
        .child(rule())
        .child(label_row)
        .child(rule())
        .into_any_element()
}

/// The compact window: the last two lines, the newest words in view.
pub fn tail(app: &App, tr: &Transcript, list: &ListState) -> El {
    let rows = list.rows(tr, false);
    let shown = rows::tail(tr, &rows);
    if shown.is_empty() {
        let body = if writing(app) {
            writing_note(app.now)
        } else {
            line(t("waitingForSpeech"), tokens::CAPTION, p().ink_muted).into_any_element()
        };
        return div().px(px(4.0)).child(body).into_any_element();
    }
    // The compact window's width is fixed; what fits beside the name is about this many ems.
    let avail = crate::app::COMPACT.width - 2.0 * 12.0 - 8.0;
    let label = |source: SourceId| -> Option<El> {
        rows.two.then(|| line(speaker_name(source), tokens::CAPTION, speaker_color(source)).medium().into_any_element())
    };
    let lines = shown.iter().filter_map(|r| -> Option<El> {
        match r.kind {
            RowKind::Partial(s) => {
                let text = tr.partials.get(&s).map_or("", |p| p.text.as_str());
                Some(tail_line(label(s), None, text, p().ink_subtle, false, avail))
            }
            RowKind::Item(i) => match &tr.items[i] {
                TimelineItem::Screenshot(_) => Some(
                    row()
                        .gap(px(6.0))
                        .h(px(24.0))
                        .child(icon(Icon::Camera, 12.0, p().ink_muted))
                        .child(line(t("screenshot"), tokens::CAPTION, p().ink_muted))
                        .into_any_element(),
                ),
                TimelineItem::Segment(s) => {
                    let star = s.important.then(|| icon(Icon::Star, 11.0, p().ink).into_any_element());
                    Some(tail_line(label(s.source), star, &s.text, p().ink, s.important, avail))
                }
                TimelineItem::Marker(_) => None,
            },
            RowKind::Header => None,
        }
    });
    col().px(px(4.0)).children(lines).into_any_element()
}

/// One line of the tail, cut at its start so the newest words stay visible.
fn tail_line(label: Option<El>, star: Option<El>, text: &str, color: Color, important: bool, avail: f32) -> El {
    let mut r = row().gap(px(8.0)).h(px(24.0));
    let mut room = avail;
    if let Some(l) = label {
        r = r.child(l);
        room -= tokens::CAPTION.size * 2.0 + 8.0;
    }
    if let Some(s) = star {
        r = r.child(s);
        room -= 19.0;
    }
    let size = tokens::BODY_SMALL.size;
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
    let text = line(shown, tokens::BODY_SMALL, color).text_right();
    let text = if important { text.medium() } else { text };
    r.child(div().flex_1().min_w_0().overflow_hidden().child(text)).into_any_element()
}
