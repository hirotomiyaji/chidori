//! The card at the top of 録音 and of a past session, and the bar at the bottom (DECISIONS → UI,
//! "The card and the bar"). While recording the card is ink: ON AIR, the title over what is
//! recorded, the time, and the live sound as bars, 相手 coral above the middle and 自分 teal below.
//! A finished session's card is white: its title, when and how long, its project, and the whole
//! session's sound with ▶ and the playhead (FR-09). The bar holds the recording's buttons
//! (スクショ, 重要, 区切り, 一時停止, 停止) or the finished session's (コピー ▾, 書き出し, ⋯). Before a
//! recording the bottom holds 開始.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use chidori_core::events::UiState;
use chidori_core::ops::CopyFormat;
use chidori_core::screenshot::WindowInfo;
use chidori_core::session::model::{SourceId, TimelineItem};
use chidori_core::settings::ScreenshotTarget;
use gpui_kit::{Bounds, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Styled, div, px};

use crate::actions::Action;
use crate::app::{App, Message};
use crate::i18n::{is_ja, t, tf};
use crate::icons::{Icon, icon};
use crate::model::format::{day_of, day_text, duration_text, sources_text, time_of, timer_text};
use crate::model::line::{Sample, Shape, session_shape};
use crate::model::transcript::Transcript;
use crate::native::menu::Item;
use crate::paint::{Frame, Point, Size as Sz, canvas_el};
use crate::screens::session_menu::{self, SessionRef};
use crate::theme::{self, Color, GRID, Ink, R_PANEL, R_TAG, alpha, g, line, p, plain, project_color, tokens};
use crate::ui::{self, El, Field, Ui, clickable, uid};
use crate::widgets::{
    Size, Variant, btn, btn_with, col, fill, icon_button, icon_button_sized, nothing, progress, round_button, row,
    spinner, swatch, with_tip,
};

#[derive(Debug, Clone)]
pub enum CardMsg {
    Seek(u64),
    Play,
    Export(String),
    ShotMenu,
    ShotWindows(Vec<WindowInfo>),
}

/// What a card keeps between frames: the whole session's shape, and where its sound was laid out
/// (for the pointer).
#[derive(Default)]
pub struct CardState {
    shape: RefCell<Option<(u64, Rc<Shape>)>>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl std::fmt::Debug for CardState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CardState")
    }
}

impl CardState {
    fn shape(&self, tr: &Transcript) -> Rc<Shape> {
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (&tr.session_id, tr.version, tr.duration_ms, tr.sound.is_some()).hash(&mut h);
            h.finish()
        };
        let mut c = self.shape.borrow_mut();
        if let Some((k, s)) = &*c
            && *k == key
        {
            return s.clone();
        }
        let s = Rc::new(session_shape(&tr.items, tr.duration_ms.unwrap_or(0), tr.sound.as_ref()));
        *c = Some((key, s.clone()));
        s
    }
}

/// The card's sound: bars 2 px wide every 4, 相手's up from the middle, 自分's down from it.
const BAR: f32 = 2.0;
const PITCH: f32 = 4.0;

/// Which sides a session recorded.
fn sides(ids: &[SourceId]) -> (bool, bool) {
    let me = ids.contains(&SourceId::Mic);
    let them = ids.iter().any(|i| *i != SourceId::Mic);
    (them, me)
}

/// The recorded sources of what the card shows: the transcript's, or the recording's.
fn source_ids(app: &App, tr: &Transcript) -> Vec<SourceId> {
    if !tr.sources.is_empty() {
        tr.sources.iter().map(|s| s.id).collect()
    } else if let Some(r) = &app.rec.sources {
        r.ids.clone()
    } else {
        Vec::new()
    }
}

/// Bars from levels 0..1, the newest at the right: `top` above the middle, `bottom` under it. With
/// one side only, its bars rise above the middle in its colour.
fn draw_bars(frame: &mut Frame, top: &[f32], bottom: &[f32], colors: (Color, Color), playhead: Option<f32>) {
    let (w, h) = (frame.width(), frame.height());
    let mid = (h / 2.0).floor();
    let n = (w / PITCH).floor() as usize;
    let up = mid - 1.0;
    let down = h - mid - 1.0;
    for i in 0..n {
        let x = i as f32 * PITCH;
        let at = |v: &[f32]| v.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        let a = (at(top) * up).round().max(1.0);
        frame.fill_rectangle(Point::new(x, mid - a), Sz::new(BAR, a), colors.0);
        if !bottom.is_empty() {
            let b = (at(bottom) * down).round().max(1.0);
            frame.fill_rectangle(Point::new(x, mid + 1.0), Sz::new(BAR, b), colors.1);
        }
    }
    if let Some(x) = playhead {
        frame.fill_rectangle(Point::new((x * w - 1.0).clamp(0.0, w - 2.0), 0.0), Sz::new(2.0, h), p().ink);
    }
}

/// The live sound's last `n` steps, one bar each, padded at the left while the recording is young.
fn live_levels(samples: &[Sample], n: usize, two: bool, me_only: bool) -> (Vec<f32>, Vec<f32>) {
    let from = samples.len().saturating_sub(n);
    let pad = n.saturating_sub(samples.len());
    let mut top = vec![0.0; pad];
    let mut bottom = vec![0.0; pad];
    for s in &samples[from..] {
        if two {
            top.push(s.o);
            bottom.push(s.m);
        } else if me_only {
            top.push(s.m);
        } else {
            top.push(s.o.max(s.m));
        }
    }
    if !two {
        bottom.clear();
    }
    (top, bottom)
}

/// A shape of any length resampled to `n` bars (the loudest slice under each).
fn resample(v: &[f32], n: usize) -> Vec<f32> {
    if v.is_empty() || n == 0 {
        return vec![0.0; n];
    }
    (0..n)
        .map(|i| {
            let a = i * v.len() / n;
            let b = (((i + 1) * v.len()).div_ceil(n)).max(a + 1).min(v.len());
            v[a..b].iter().copied().fold(0.0, f32::max)
        })
        .collect()
}

/// The live sound, `width` × `height` (the card's 352 × 40, the compact window's 120 × 40).
pub fn live_wave(app: &App, tr: &Transcript, width: Option<f32>, height: f32) -> El {
    let (them, me) = sides(&source_ids(app, tr));
    let two = them && me;
    let samples = app.line.samples.clone();
    let paused = app.rec.state == UiState::Paused;
    let p_ = p();
    let colors = if paused {
        (p_.faint, p_.faint)
    } else if !two && me {
        (p_.me_wave, p_.me_wave)
    } else {
        (p_.them_wave, p_.me_wave)
    };
    let el = canvas_el(None, move |frame| {
        let n = (frame.width() / PITCH).floor() as usize;
        let (top, bottom) = live_levels(&samples, n, two, !them && me);
        draw_bars(frame, &top, &bottom, colors, None);
    })
    .h(px(height));
    match width {
        Some(w) => el.flex_none().w(px(w)).into_any_element(),
        None => el.w_full().into_any_element(),
    }
}

/// The note on the card after what is recorded: 遅れ n秒 / 一時停止中 / 停止後に文字起こし.
/// Quiet while it may catch up, bold from 15 s, coral from 30 s (FR-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Quiet,
    Bold,
    Warn,
}

/// The note, from the pause, the deferral and the lag in ms.
fn note_of(paused: bool, deferred: bool, lag_ms: u64) -> Option<(String, Tone)> {
    let lag = ((lag_ms as f64) / 1000.0).round() as u64;
    let text = if paused {
        t("paused")
    } else if deferred {
        // Waiting for Stop, the lag only grows and says nothing (FR-25).
        t("deferredNote")
    } else if lag >= 2 {
        tf("lag", &[("n", &lag)])
    } else {
        return None;
    };
    let tone = if paused || deferred || lag < 15 {
        Tone::Quiet
    } else if lag < 30 {
        Tone::Bold
    } else {
        Tone::Warn
    };
    Some((text, tone))
}

/// The ink card while recording.
pub fn live_card(app: &App, tr: &Transcript, title_input: Option<El>, rename: Message) -> El {
    let p_ = p();
    let paused = app.rec.state == UiState::Paused;
    let tag = div()
        .flex()
        .flex_none()
        .items_center()
        .h(g(4.0))
        .px(g(2.0))
        .rounded(px(R_TAG))
        .bg(if paused { p_.faint } else { p_.on_ink })
        .text_color(p_.ink)
        .text_size(px(tokens::LABEL.size))
        .font_weight(gpui_kit::FontWeight::BLACK)
        .child(t("onAir"));
    let tag = with_tip(tag, t(if paused { "paused" } else { "recording" }));

    let mut sub = row().gap(g(1.0)).min_w_0().overflow_hidden();
    let what = sources_text(app.rec.sources.as_ref());
    // What is recorded switches from a menu (FR-17), as in the compact window.
    let faint = p_.faint;
    let on_ink = p_.on_ink;
    let switch = clickable(Some(Message::Titlebar(crate::screens::titlebar::Msg::SourceMenu)), "card-source")
        .flex_none()
        .text_color(faint)
        .hover(move |s| s.text_color(on_ink))
        .child(plain(what, tokens::SMALL));
    sub = sub.child(with_tip(switch, t("chooseSource")));
    let rec = &app.rec;
    if let Some((text, tone)) = note_of(paused, rec.deferred, rec.lag.as_ref().map_or(0, |l| l.lag_ms)) {
        let color = match tone {
            Tone::Warn => p_.them_wave,
            Tone::Bold => p_.on_ink,
            Tone::Quiet => p_.faint,
        };
        let text = line(format!("· {text}"), tokens::SMALL, color);
        sub = sub.child(if tone == Tone::Quiet { text } else { text.bold() });
        if rec.deferred && !paused {
            sub = sub.child(
                clickable(Some(Message::Action(Action::DeferTranscription(false))), "resume-transcription")
                    .child(line(t("resumeTranscription"), tokens::SMALL, p_.on_ink).bold().underline()),
            );
        }
    }

    let title: El = match title_input {
        Some(input) => input,
        None => {
            let name = tr.title.clone().unwrap_or_default();
            let b = clickable(Some(rename), "card-title")
                .min_w_0()
                .overflow_hidden()
                .child(line(name, tokens::BODY, p_.on_ink).bold());
            with_tip(b, t("rename")).into_any_element()
        }
    };
    let words = col().flex_1().min_w_0().child(title).child(sub);
    let timer = with_tip(
        line(timer_text(app.rec.elapsed(app.now)), tokens::TIMER, if paused { p_.faint } else { p_.on_ink }).mono(),
        t("elapsed"),
    );
    let head = row().h(g(5.0)).gap(g(2.0)).child(tag).child(words).child(timer);
    col()
        .w_full()
        .gap(g(2.0))
        .p(g(2.0))
        .rounded(px(R_PANEL))
        .bg(p_.ink)
        .text_color(p_.on_ink)
        .child(head)
        .child(live_wave(app, tr, None, 5.0 * GRID))
        .into_any_element()
}

/// What a finished session's card and bar need.
#[derive(Clone, Copy)]
pub struct SessionCtx<'a> {
    pub app: &'a App,
    pub tr: &'a Transcript,
    pub state: &'a CardState,
    /// The session kept its sound (FR-09).
    pub player: bool,
    pub playing: bool,
    pub playing_ms: Option<u64>,
    /// 「新しい録音」 in the bar (the main window only).
    pub on_new: bool,
    /// A way back to 履歴 (a past session opened from it).
    pub back: bool,
    pub wrap: fn(CardMsg) -> Message,
}

/// The session's length on its line: its duration, or its last line's end.
fn total(tr: &Transcript) -> u64 {
    let mut total = tr.duration_ms.unwrap_or(0);
    for i in &tr.items {
        if let TimelineItem::Segment(s) = i {
            total = total.max(s.t_end_ms);
        }
    }
    total
}

/// 「11月8日 22:00 · 41分 · Zoom + マイク」.
fn session_meta(tr: &Transcript) -> String {
    let mut parts = Vec::new();
    if let Some(s) = &tr.started_at {
        let day = day_text(day_of(s), chrono::Datelike::year(&chrono::Local::now()));
        // The weekday in brackets is for lists; a card has room for the time instead.
        let day = day.split('（').next().unwrap_or(&day).to_string();
        parts.push(if is_ja() { format!("{day} {}", time_of(s)) } else { format!("{day}, {}", time_of(s)) });
    }
    let len = total(tr);
    if len > 0 {
        parts.push(duration_text(len));
    }
    let names: Vec<String> = tr
        .sources
        .iter()
        .map(|s| match s.id {
            SourceId::Mic => t("srcMic"),
            SourceId::System => t("srcSystem"),
            _ => s.name.clone().unwrap_or_else(|| s.label.clone()),
        })
        .collect();
    if !names.is_empty() {
        parts.push(names.join(" + "));
    }
    parts.join(" · ")
}

/// Everyday and full name of the model a session was written with, and on what it ran.
fn model_text(app: &App, tr: &Transcript) -> String {
    let mut parts = Vec::new();
    if let Some(id) = &tr.model_id {
        let full = app
            .models
            .iter()
            .find(|m| &m.entry.id == id)
            .map_or_else(|| id.clone(), |m| if is_ja() { m.entry.name_ja.clone() } else { m.entry.name_en.clone() });
        parts.push(full);
    }
    if let Some(gpu) = tr.gpu {
        parts.push(t(if gpu { "gpu" } else { "cpu" }));
    }
    parts.join(" · ")
}

/// The session's project as a chip that opens the project menu (FR-64).
fn project_chip(app: &App, tr: &Transcript) -> El {
    let Some(id) = tr.session_id.clone() else { return nothing() };
    let project = tr.project.as_deref().and_then(|pid| app.projects.iter().find(|p| p.id == pid));
    let menu = Message::Popup(session_menu::project_items(app, &[id], tr.project.as_deref()), None);
    let p_ = p();
    let mut content = row().gap(g(1.0));
    let label = match project {
        Some(pr) => {
            content = content.child(swatch(project_color(&pr.color), GRID));
            line(pr.name.clone(), tokens::SMALL, p_.ink)
        }
        None => line(t("noProject"), tokens::SMALL, p_.muted),
    };
    content = content.child(label.max_w(g(14.0)).overflow_hidden());
    let hover = p_.line;
    let b = clickable(Some(menu), "project-chip")
        .flex()
        .flex_none()
        .items_center()
        .h(g(4.0))
        .px(g(2.0))
        .rounded(g(2.0))
        .bg(p_.ground)
        .hover(move |s| s.bg(hover))
        .child(content);
    with_tip(b, t("project")).into_any_element()
}

/// The finished session's sound across the card, with the playhead; a click plays from there, or
/// jumps the transcript there when there is no sound to play.
fn session_wave(c: SessionCtx<'_>) -> El {
    let tr = c.tr;
    let len = total(tr);
    let shape = c.state.shape(tr);
    let (them, me) = sides(&source_ids(c.app, tr));
    let two = them && me;
    let p_ = p();
    let colors = if !two && me { (p_.me_wave, p_.me_wave) } else { (p_.them_wave, p_.me_wave) };
    let playhead = c.playing_ms.filter(|_| len > 0).map(|ms| (ms as f32 / len as f32).min(1.0));
    let canvas = canvas_el(Some(c.state.bounds.clone()), move |frame| {
        let n = (frame.width() / PITCH).floor() as usize;
        let (top, bottom) = if two {
            (resample(&shape.o, n), resample(&shape.m, n))
        } else {
            let both: Vec<f32> = (0..shape.o.len().max(shape.m.len()))
                .map(|i| shape.o.get(i).copied().unwrap_or(0.0).max(shape.m.get(i).copied().unwrap_or(0.0)))
                .collect();
            (resample(&both, n), Vec::new())
        };
        draw_bars(frame, &top, &bottom, colors, playhead);
    })
    .size_full();
    let mut area = div().id(uid("card-wave")).flex_1().min_w_0().h(g(5.0)).child(canvas);
    if len > 0 {
        let bounds = c.state.bounds.clone();
        let wrap = c.wrap;
        area = area.cursor_pointer().on_mouse_down(MouseButton::Left, move |e, _, cx| {
            let b = bounds.get();
            let w = f32::from(b.size.width);
            if w > 0.0 {
                let x = (f32::from(e.position.x - b.origin.x) / w).clamp(0.0, 1.0);
                ui::send(wrap(CardMsg::Seek((x * len as f32) as u64)), cx);
            }
        });
    }
    area.into_any_element()
}

/// The finished session's white card. `title_input` is the box to rename it in, while open.
pub fn session_card(c: SessionCtx<'_>, title_input: Option<El>, rename: Message) -> El {
    let p_ = p();
    let tr = c.tr;
    let title: El = match title_input {
        Some(input) => input,
        None => {
            let b = clickable(Some(rename), "session-title")
                .min_w_0()
                .overflow_hidden()
                .child(line(tr.title.clone().unwrap_or_default(), tokens::BODY, p_.ink).bold());
            with_tip(b, t("rename")).into_any_element()
        }
    };
    let model = model_text(c.app, tr);
    let meta = with_tip(line(session_meta(tr), tokens::SMALL, p_.muted).min_w_0(), model);
    let mut head = row().h(g(5.0)).gap(g(2.0));
    if c.back {
        let back = Message::Go(crate::app::View::History);
        head = head.child(icon_button(Icon::ArrowLeft, t("back"), Size::Md, Some(back)));
    }
    head =
        head.child(col().flex_1().min_w_0().overflow_hidden().child(title).child(meta)).child(project_chip(c.app, tr));
    let mut card = col().w_full().gap(g(2.0)).p(g(2.0)).rounded(px(R_PANEL)).bg(p_.surface).child(head);
    let len = total(tr);
    if len > 0 || c.player {
        let mut r = row().h(g(5.0)).gap(g(2.0));
        if c.player {
            let (glyph, tip) = if c.playing { (Icon::Pause, t("pausePlayback")) } else { (Icon::Play, t("play")) };
            let play =
                round_button(icon(glyph, 16.0, p_.on_ink), Variant::Primary, Size::Md, Some((c.wrap)(CardMsg::Play)));
            r = r.child(with_tip(play, tip));
        }
        card = card.child(r.child(session_wave(c)));
    }
    card.into_any_element()
}

/// コピー: an ink pill split in two, the words copying plain text and ▾ picking another format.
fn copy_split(id: &str) -> El {
    let p_ = p();
    let menu = vec![
        Item::action(format!("action:copy-agent:{id}"), t("copyForAgent")),
        Item::action(format!("action:copy-markdown:{id}"), t("copyMarkdown")),
    ];
    let hover = theme::mix(p_.ink, p_.on_ink, 0.12);
    let main = clickable(Some(Message::Action(Action::Copy(CopyFormat::Plain, Some(id.to_string())))), "copy")
        .flex()
        .items_center()
        .h_full()
        .pl(g(3.0))
        .pr(g(2.0))
        .rounded_l(g(3.0))
        .hover(move |s| s.bg(hover))
        .child(plain(t("copy"), tokens::LABEL).bold());
    let more = clickable(Some(Message::Popup(menu, None)), "copy-as")
        .flex()
        .items_center()
        .justify_center()
        .w(g(5.0))
        .h_full()
        .rounded_r(g(3.0))
        .hover(move |s| s.bg(hover))
        .child(icon(Icon::ChevronDown, 16.0, p_.on_ink));
    row()
        .flex_none()
        .h(g(6.0))
        .rounded(g(3.0))
        .bg(p_.ink)
        .text_color(p_.on_ink)
        .child(main)
        .child(div().w(px(1.0)).h(g(2.0)).bg(alpha(p_.on_ink, 0.25)))
        .child(with_tip(more, t("copyAs")))
        .into_any_element()
}

/// The bottom bar: 80 high, its buttons 48, on the grid.
pub fn bar(content: impl IntoElement) -> El {
    row().flex_none().w_full().h(g(10.0)).px(g(2.0)).pb(g(1.0)).gap(g(1.0)).child(content).into_any_element()
}

/// The finished session's bar: コピー ▾, 書き出し, then 新しい録音 and ⋯.
pub fn review_bar(c: SessionCtx<'_>) -> El {
    let Some(id) = c.tr.session_id.clone() else { return nothing() };
    let more =
        session_menu::items(c.app, SessionRef { id: &id, recoverable: false, project: c.tr.project.as_deref() }, false);
    let mut r = row()
        .w_full()
        .gap(g(1.0))
        .child(copy_split(&id))
        .child(btn(t("export"), Variant::Secondary, Size::Lg, Some((c.wrap)(CardMsg::Export(id.clone())))))
        .child(fill());
    if c.on_new {
        r = r.child(new_button(48.0));
    }
    r = r.child(icon_button(Icon::Ellipsis, t("moreActions"), Size::Lg, Some(Message::Popup(more, None))));
    bar(r)
}

/// 「新しい録音」: an icon button with the record dot, back to the start.
fn new_button(side: f32) -> El {
    let dot = div().size(g(2.0)).rounded(g(1.0)).bg(p().rec);
    let hover = p().hover;
    let b = clickable(Some(Message::Action(Action::NewRecording)), "new-recording")
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded(px(side / 2.0))
        .hover(move |s| s.bg(hover))
        .child(dot);
    with_tip(b, t("newRecording")).into_any_element()
}

/// 停止: the ink pill with the red square.
fn stop_button(height: f32, square: f32, compact: bool) -> El {
    let label = plain(t("stop"), tokens::LABEL).bold();
    let content =
        row().gap(g(1.0)).child(div().flex_none().size(px(square)).rounded(px(square / 4.0)).bg(p().rec)).child(label);
    let b = btn_with(
        content,
        Variant::Primary,
        if compact { Size::Md } else { Size::Lg },
        Some(Message::Action(Action::Stop)),
    )
    .h(px(height));
    with_tip(b, t("stop")).into_any_element()
}

/// The camera, and the small chevron beside it for what it takes (FR-34).
fn camera(app: &App, wrap: fn(CardMsg) -> Message, side: f32, menu: bool) -> El {
    let paused = app.rec.state == UiState::Paused;
    let tip = match &app.rec.shot_window {
        Some(w) => format!("{}: {}", t("screenshot"), w.title),
        None => t("screenshot"),
    };
    let glyph = if side >= 48.0 { 24.0 } else { 16.0 };
    let shot = icon_button_sized(
        Icon::Camera,
        tip,
        side,
        side,
        glyph,
        (!paused).then_some(Message::Action(Action::Screenshot)),
        None,
    );
    if !menu {
        return shot;
    }
    let chevron = icon_button_sized(
        Icon::ChevronDown,
        t("shotTarget"),
        2.0 * GRID,
        side,
        16.0,
        Some(wrap(CardMsg::ShotMenu)),
        None,
    );
    row().flex_none().child(shot).child(chevron).into_any_element()
}

/// The recording's bar: スクショ ▾, 重要, 区切り, then 一時停止 and 停止.
pub fn live_bar(app: &App, wrap: fn(CardMsg) -> Message) -> El {
    let paused = app.rec.state == UiState::Paused;
    let pause = if paused {
        icon_button_sized(
            Icon::Play,
            t("resume"),
            48.0,
            48.0,
            24.0,
            Some(Message::Action(Action::PauseResume)),
            Some(p().ink),
        )
    } else {
        icon_button(Icon::Pause, t("pause"), Size::Lg, Some(Message::Action(Action::PauseResume)))
    };
    bar(row()
        .w_full()
        .gap(g(1.0))
        .child(camera(app, wrap, 48.0, true))
        .child(icon_button(Icon::Star, t("markLine"), Size::Lg, Some(Message::Action(Action::MarkLine))))
        .child(icon_button(Icon::Scissors, t("cut"), Size::Lg, Some(Message::Action(Action::Cut))))
        .child(fill())
        .child(pause)
        .child(stop_button(48.0, 16.0, false)))
}

/// While the last words are written after 停止: what is left, and a way to stop writing.
pub fn finishing_bar(app: &App) -> El {
    let f = app.rec.finishing.as_ref();
    // A file (FR-18) shows how far into it the text has got, from the start.
    let from_file = app.rec.sources.as_ref().is_some_and(|s| s.ids.contains(&SourceId::File));
    let mut words = row()
        .min_w_0()
        .overflow_hidden()
        .child(line(t(if from_file { "transcribingFile" } else { "finishing" }), tokens::SMALL, p().ink).bold());
    if let Some(f) = f.filter(|f| from_file && f.total_ms > 0) {
        words = words.child(
            line(format!(" · {} / {}", timer_text(f.done_ms), timer_text(f.total_ms)), tokens::SMALL, p().muted).mono(),
        );
    }
    if let Some(f) = f.filter(|f| f.total > 0) {
        words = words.child(line(
            format!(" · {}", tf("finishingLeft", &[("n", &(f.total - f.done))])),
            tokens::SMALL,
            p().muted,
        ));
    }
    let mut left = col().gap(g(1.0)).child(row().gap(g(1.0)).child(spinner(16.0, app.now)).child(words));
    // By audio rather than by line: the lines left differ in length, merged ones most.
    if let Some(f) = f.filter(|f| f.total_ms > 0) {
        left = left.child(progress(f.done_ms as f32, f.total_ms as f32));
    }
    let left: El =
        if from_file { left.into_any_element() } else { with_tip(left, t("finishingDetail")).into_any_element() };
    bar(row().w_full().gap(g(2.0)).child(div().flex_1().min_w_0().child(left)).child(btn(
        t("stopFinishing"),
        Variant::Secondary,
        Size::Lg,
        Some(Message::Action(Action::CancelFinishing)),
    )))
}

/// 「開始 Ctrl+Shift+R」, without the key when none is set or another app holds it.
fn start_label(app: &App) -> El {
    let mut r = row().gap(g(1.0)).child(line(t("start"), tokens::LABEL, p().ink).bold());
    let key = app.settings.hotkeys.toggle.trim();
    if !key.is_empty() && !app.info.hotkey_errors.contains_key("toggle") {
        r = r.child(line(key.to_string(), tokens::LABEL, p().muted));
    }
    r.into_any_element()
}

/// 開始: round and ink, with the record dot.
fn go_button(app: &App, side: f32, dot: f32) -> El {
    let enabled = app.rec.state == UiState::Ready;
    let p_ = p();
    let hover = theme::mix(p_.ink, p_.on_ink, 0.12);
    let mut b = clickable(enabled.then_some(Message::Action(Action::ToggleRecording)), "go")
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded(px(side / 2.0))
        .bg(p_.ink)
        .child(div().size(px(dot)).rounded(px(dot / 2.0)).bg(p_.rec));
    if enabled {
        b = b.hover(move |s| s.bg(hover));
    } else {
        b = b.opacity(0.45);
    }
    with_tip(b, t("start")).into_any_element()
}

/// The start screen's foot: 開始, its key, ファイルから文字起こし and the model.
pub fn start_foot(app: &App, file: El) -> El {
    let mut c = col()
        .flex_none()
        .w_full()
        .h(g(23.0))
        .items_center()
        .justify_center()
        .gap(g(1.0))
        .child(go_button(app, 9.0 * GRID, 3.0 * GRID))
        .child(start_label(app))
        .child(file);
    if let Some(note) = crate::screens::models::note(app) {
        c = c.child(note);
    }
    c.into_any_element()
}

/// The compact window before a recording: 開始 and the source.
pub fn compact_start(app: &App) -> El {
    row()
        .flex_none()
        .gap(g(1.0))
        .child(go_button(app, 5.0 * GRID, 2.0 * GRID))
        .child(start_label(app))
        .into_any_element()
}

/// The compact window's third row while recording: the dot, the time, the sound, the camera and 停止.
pub fn compact_live(app: &App, tr: &Transcript, wrap: fn(CardMsg) -> Message) -> El {
    let paused = app.rec.state == UiState::Paused;
    let dot = div().flex_none().size(g(2.0)).rounded(g(1.0)).bg(if paused { p().faint } else { p().rec });
    let timer =
        line(timer_text(app.rec.elapsed(app.now)), tokens::TIMER_SMALL, if paused { p().muted } else { p().ink })
            .mono()
            .flex_none()
            .min_w(g(6.0));
    let pause_msg = Some(Message::Action(Action::PauseResume));
    let pause = if paused {
        icon_button_sized(Icon::Play, t("resume"), 40.0, 40.0, 16.0, pause_msg, Some(p().ink))
    } else {
        icon_button_sized(Icon::Pause, t("pause"), 40.0, 40.0, 16.0, pause_msg, None)
    };
    row()
        .w_full()
        .gap(g(1.0))
        .child(dot)
        .child(with_tip(timer, t("elapsed")))
        .child(live_wave(app, tr, Some(15.0 * GRID), 5.0 * GRID))
        .child(fill())
        .child(camera(app, wrap, 40.0, false))
        .child(pause)
        .child(stop_button(40.0, 8.0, true))
        .into_any_element()
}

/// The compact window's third row after 停止, while the last words are written.
pub fn compact_finishing(app: &App) -> El {
    let f = app.rec.finishing.as_ref();
    let mut r = row()
        .w_full()
        .gap(g(1.0))
        .child(spinner(16.0, app.now))
        .child(line(t("finishing"), tokens::SMALL, p().ink).bold());
    if let Some(f) = f.filter(|f| f.total > 0) {
        r = r.child(line(tf("finishingLeft", &[("n", &(f.total - f.done))]), tokens::SMALL, p().muted));
    }
    r.child(fill())
        .child(btn(t("stopFinishing"), Variant::Secondary, Size::Md, Some(Message::Action(Action::CancelFinishing))))
        .into_any_element()
}

/// The compact window's third row for a finished session: ▶, コピー, 書き出し, 新しい録音 and ⋯.
pub fn compact_review(c: SessionCtx<'_>) -> El {
    let Some(id) = c.tr.session_id.clone() else { return nothing() };
    let mut r = row().w_full().gap(g(1.0));
    if c.player {
        let (glyph, tip) = if c.playing { (Icon::Pause, t("pausePlayback")) } else { (Icon::Play, t("play")) };
        let play =
            round_button(icon(glyph, 16.0, p().on_ink), Variant::Primary, Size::Md, Some((c.wrap)(CardMsg::Play)));
        r = r.child(with_tip(play, tip));
    }
    // 書き出し picks a format in a dialog; the compact window is too small for one.
    let export = vec![
        Item::action(format!("action:export-markdown:{id}"), t("exportMarkdown")),
        Item::action(format!("action:export-zip:{id}"), t("exportMarkdownZip")),
        Item::action(format!("action:export-pdf:{id}"), t("exportPdf")),
        Item::action(format!("action:export-typst:{id}"), t("exportTypst")),
    ];
    let copy = vec![
        Item::action(format!("action:copy-plain:{id}"), t("copy")),
        Item::action(format!("action:copy-agent:{id}"), t("copyForAgent")),
        Item::action(format!("action:copy-markdown:{id}"), t("copyMarkdown")),
    ];
    let more =
        session_menu::items(c.app, SessionRef { id: &id, recoverable: false, project: c.tr.project.as_deref() }, false);
    r.child(icon_button(Icon::Copy, t("copy"), Size::Md, Some(Message::Popup(copy, None))))
        .child(icon_button(Icon::FileOutput, t("export"), Size::Md, Some(Message::Popup(export, None))))
        .child(fill())
        .child(new_button(40.0))
        .child(icon_button(Icon::Ellipsis, t("moreActions"), Size::Md, Some(Message::Popup(more, None))))
        .into_any_element()
}

/// The camera's menu (FR-34): the recorded app's window, the cursor's screen or every screen (the
/// setting, remembered), or any open window for the rest of this recording.
pub fn shot_menu(app: &App, mut windows: Vec<WindowInfo>) -> Vec<Item> {
    let target = app.settings.screenshot.target;
    let picked = app.rec.shot_window.clone();
    let recorded_app =
        app.rec.sources.as_ref().filter(|s| s.ids.contains(&SourceId::App)).and_then(|s| s.app_name.clone());
    if let Some(w) = &picked
        && !windows.iter().any(|x| x.id == w.id)
    {
        windows.insert(0, w.clone());
    }
    // Without an app recorded, the app's window means the cursor's screen.
    let cursor =
        target == ScreenshotTarget::CursorMonitor || (target == ScreenshotTarget::AppWindow && recorded_app.is_none());
    let mut items = Vec::new();
    if let Some(a) = &recorded_app {
        items.push(Item::check(
            "main:shot-target:appWindow",
            menu_text(&tf("shotWindowOf", &[("app", a)])),
            picked.is_none() && target == ScreenshotTarget::AppWindow,
        ));
    }
    items.push(Item::check("main:shot-target:cursorMonitor", t("targetCursor"), picked.is_none() && cursor));
    items.push(Item::check(
        "main:shot-target:allMonitors",
        t("targetAll"),
        picked.is_none() && target == ScreenshotTarget::AllMonitors,
    ));
    items.push(Item::Separator);
    let sub = if windows.is_empty() {
        vec![Item::action("main:none", t("noWindows")).enabled(false)]
    } else {
        windows
            .iter()
            .map(|w| {
                let title = if w.title.chars().count() > 60 {
                    format!("{}…", w.title.chars().take(59).collect::<String>())
                } else {
                    w.title.clone()
                };
                Item::check(
                    format!("main:shot-window:{}", w.id),
                    menu_text(&title),
                    picked.as_ref().is_some_and(|p| p.id == w.id),
                )
            })
            .collect()
    };
    items.push(Item::submenu(t("shotPickWindow"), sub));
    items
}

/// Text for a native menu, where `&` marks a shortcut letter.
pub fn menu_text(s: &str) -> String {
    s.replace('&', "&&")
}

/// The title's box while it is renamed, with its save button.
pub fn title_box(
    ui: &mut Ui,
    key: gpui_kit::SharedString,
    draft: &str,
    on_input: impl Fn(String) -> Message + 'static,
    save: Message,
) -> El {
    let input = ui.input(Field::new(key, draft, on_input).placeholder(t("newTitle")).on_submit(save.clone()));
    row()
        .w_full()
        .gap(g(1.0))
        .child(div().flex_1().min_w_0().child(input))
        .child(btn(t("save"), Variant::Secondary, Size::Sm, Some(save)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_says_how_far_behind_from_two_seconds_on() {
        crate::i18n::set_locale(chidori_core::settings::Locale::Ja);
        assert_eq!(note_of(false, false, 1400), None);
        assert_eq!(note_of(false, false, 20_000), Some(("遅れ 20秒".into(), Tone::Bold)));
        assert_eq!(note_of(false, false, 5_000).map(|n| n.1), Some(Tone::Quiet));
        assert_eq!(note_of(false, false, 45_000).map(|n| n.1), Some(Tone::Warn));
        assert_eq!(note_of(false, true, 60_000), Some(("停止後に文字起こし".into(), Tone::Quiet)));
        assert_eq!(note_of(true, false, 60_000), Some(("一時停止中".into(), Tone::Quiet)));
    }

    #[test]
    fn native_menus_show_ampersands() {
        assert_eq!(menu_text("Q&A"), "Q&&A");
    }

    #[test]
    fn a_young_recording_fills_the_bars_from_the_right() {
        let s = |o: f32, m: f32| Sample { o, m, ..Default::default() };
        let (top, bottom) = live_levels(&[s(0.5, 0.1), s(0.2, 0.9)], 4, true, false);
        assert_eq!(top, vec![0.0, 0.0, 0.5, 0.2]);
        assert_eq!(bottom, vec![0.0, 0.0, 0.1, 0.9]);
        // One side: its bars rise above the middle, whichever side it is.
        let (top, bottom) = live_levels(&[s(0.0, 0.7)], 2, false, true);
        assert_eq!((top, bottom.len()), (vec![0.0, 0.7], 0));
    }

    #[test]
    fn a_session_resamples_to_its_loudest_slices() {
        assert_eq!(resample(&[0.1, 0.5, 0.2, 0.9], 2), vec![0.5, 0.9]);
        assert_eq!(resample(&[0.4], 3), vec![0.4, 0.4, 0.4]);
        assert_eq!(resample(&[], 2), vec![0.0, 0.0]);
    }
}
