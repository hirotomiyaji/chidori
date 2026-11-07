//! The bottom of the main window and of a past session: one line across the window (DECISIONS → UI,
//! "The line is real sound", "Every session keeps its shape", "開始 is the primary button"). Before
//! a recording 開始 waits in the middle of the free space (in the compact window, at the start of
//! an empty line). Pressing it brings the line down to the bottom with a pen tip that draws it as
//! it moves: 相手 above the line in the accent, 自分 below it in ink, over the accent's tint. When
//! the session is saved the pen lifts and the line zooms out to the whole session, which you can
//! point at for the time and click to jump there; the finished session's buttons sit under it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use chidori_core::events::UiState;
use chidori_core::ops::CopyFormat;
use chidori_core::screenshot::WindowInfo;
use chidori_core::session::model::{SourceId, TimelineItem};
use chidori_core::settings::{ScreenshotTarget, SourceMode};
use gpui_kit::{
    Bounds, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, StatefulInteractiveElement, Styled,
    div, px,
};

use super::live_line::{self, Geometry, build};
use crate::actions::Action;
use crate::app::{App, Message};
use crate::i18n::{t, tf};
use crate::icons::{Icon, icon};
use crate::model::format::timer_text;
use crate::model::line::{Shape, session_shape};
use crate::model::transcript::Transcript;
use crate::native::menu::Item;
use crate::paint::{self, Frame, LineCap, LineDash, LineJoin, Path, Point, Size as Sz, Stroke, canvas_el};
use crate::screens::session_line::{MapGeometry, map_line};
use crate::screens::session_menu::{self, SessionRef};
use crate::theme::{self, Color, Ink, alpha, hint, line, mix, p, plain, tokens};
use crate::ui::{El, clickable, uid};
use crate::widgets::{Size, Variant, btn_with, col, fill, nothing, progress, row, spinner, with_tip};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeckState {
    Start,
    Recording,
    Paused,
    Finishing,
    Review,
}

/// The deck's sizes: its height while recording; the baseline then and, in the compact window,
/// before (the expanded window centres 開始 in the free space instead, with no line until it
/// starts); how far each side reaches; one step's width; where the pen starts (in line with the
/// buttons under it) and where it stops; where the compact 開始 sits; the pen's radius; the
/// whole session's height; the line's weight and a screenshot dot's radius.
#[derive(Debug, Clone, Copy)]
pub struct Sizes {
    pub height: f32,
    pub base: f32,
    start_base: f32,
    centred: bool,
    up: f32,
    down: f32,
    step: f32,
    start_x: f32,
    end_pad: f32,
    dot_x: f32,
    pen: f32,
    map_amp: f32,
    stroke: f32,
    shot_r: f32,
}

pub const EXPANDED: Sizes = Sizes {
    height: 124.0,
    base: super::transcript::LINE_Y,
    start_base: 30.0,
    centred: true,
    up: 20.0,
    down: 15.0,
    step: 1.6,
    start_x: 14.0,
    end_pad: 44.0,
    dot_x: 14.0,
    pen: 4.5,
    map_amp: 0.62,
    stroke: 2.6,
    shot_r: 3.0,
};

pub const COMPACT: Sizes = Sizes {
    height: 66.0,
    base: 12.0,
    start_base: 30.0,
    centred: false,
    up: 10.0,
    down: 8.0,
    step: 1.4,
    start_x: 12.0,
    end_pad: 34.0,
    dot_x: 12.0,
    pen: 3.5,
    map_amp: 0.75,
    stroke: 2.2,
    shot_r: 2.5,
};

/// Where the expanded 開始 rests, as a share of the deck's height above the model's line at the
/// bottom (`START_FOOT` px): a little above the middle, at the optical centre, because a shape at
/// the exact middle looks low.
const START_FOOT: f32 = 30.0;
const START_LIFT: f32 = 0.45;

/// Finishing this long shows a progress bar; a short one is over before a bar would mean anything.
const FINISHING_BAR: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anim {
    /// 開始: the line sinks to the bottom while the pen comes in at its start (480 ms).
    Starting,
    /// Saved: the pen lifts (260 ms), then the line zooms out to the whole session (800 ms).
    Settling,
    /// 「新しい録音」: the session's line flattens (300 ms), then rises as the pen fades (480 ms).
    Rising,
}

impl Anim {
    fn total(self) -> Duration {
        Duration::from_millis(match self {
            Anim::Starting => 480,
            Anim::Settling => 1060,
            Anim::Rising => 780,
        })
    }
}

/// The deck's moving parts, kept between frames: what it last drew, a move under way, and the
/// whole session's shape.
#[derive(Default)]
pub struct DeckAnim {
    drawn: Cell<Option<DeckState>>,
    anim: Cell<Option<(Anim, Instant)>>,
    pending_from: Cell<usize>,
    finishing_since: Cell<Option<Instant>>,
    /// The start deck's full height when last laid out, for easing to and from it.
    full_h: Cell<f32>,
    shape: RefCell<Option<(u64, Rc<Shape>)>>,
    /// Where the line was laid out, for the pointer.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The pointer on a finished session's line, as a share of its width.
    scrub: Rc<Cell<Option<f32>>>,
}

impl std::fmt::Debug for DeckAnim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeckAnim").field("drawn", &self.drawn.get()).finish()
    }
}

impl DeckAnim {
    /// The deck is off screen: when it comes back it shows its state without moving to it.
    pub fn forget(&self) {
        self.drawn.set(None);
        self.anim.set(None);
        self.scrub.set(None);
    }

    /// Notes the state about to be drawn, starting a move when it changed in view.
    pub fn observe(&self, state: DeckState, samples: usize) {
        let from = self.drawn.replace(Some(state));
        if state == DeckState::Finishing {
            if from != Some(DeckState::Finishing) {
                self.pending_from.set(samples.min(40));
                self.finishing_since.set(Some(Instant::now()));
            }
        } else {
            self.finishing_since.set(None);
        }
        let Some(from) = from else { return };
        if from == state {
            return;
        }
        let anim = match (from, state) {
            (DeckState::Start, _) => Some(Anim::Starting),
            (DeckState::Review, DeckState::Start) => Some(Anim::Rising),
            (DeckState::Recording | DeckState::Paused | DeckState::Finishing, DeckState::Review) => {
                Some(Anim::Settling)
            }
            _ => None,
        };
        self.anim.set(anim.map(|a| (a, Instant::now())));
    }

    /// The state last drawn.
    pub fn drawn_state(&self) -> Option<DeckState> {
        self.drawn.get()
    }

    /// Whether a move is under way.
    pub fn moving(&self, now: Instant) -> bool {
        self.anim.get().is_some_and(|(a, at)| now.saturating_duration_since(at) < a.total())
    }

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

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 { 2.0 * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(2) / 2.0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Dot,
    Live,
    Map,
}

/// Where the line is in its moves.
#[derive(Debug, Clone, Copy)]
struct Vals {
    mode: Mode,
    /// 0 the line as it waits before a recording … 1 where it is while recording.
    mv: f32,
    /// The pen's radius as it comes in with the line, and while drawing.
    dot_r: f32,
    pen_r: f32,
    /// 0 the live line … 1 the whole session.
    k: f32,
    /// The whole session's heights: 1 at rest, 0 flat.
    amp: f32,
    /// How far the deck has eased from its strip (0) to the full height (1), while it moves.
    full: Option<f32>,
}

fn vals(anim: &DeckAnim, state: DeckState, s: &Sizes, now: Instant) -> Vals {
    let rest = match state {
        DeckState::Start => Vals { mode: Mode::Dot, mv: 0.0, dot_r: 0.0, pen_r: 0.0, k: 0.0, amp: 1.0, full: None },
        DeckState::Review => Vals { mode: Mode::Map, mv: 1.0, dot_r: 0.0, pen_r: 0.0, k: 1.0, amp: 1.0, full: None },
        _ => Vals { mode: Mode::Live, mv: 1.0, dot_r: 0.0, pen_r: s.pen, k: 0.0, amp: 1.0, full: None },
    };
    let Some((a, at)) = anim.anim.get() else { return rest };
    let ms = now.saturating_duration_since(at).as_secs_f32() * 1000.0;
    if ms >= a.total().as_secs_f32() * 1000.0 {
        return rest;
    }
    match a {
        Anim::Starting => {
            let p = ease(ms / 480.0);
            Vals { mode: Mode::Dot, mv: p, dot_r: s.pen * p, full: Some(1.0 - p), ..rest }
        }
        Anim::Settling if ms < 260.0 => {
            Vals { mode: Mode::Live, pen_r: s.pen * (1.0 - ease(ms / 260.0)), k: 0.0, ..rest }
        }
        Anim::Settling => Vals { mode: Mode::Live, pen_r: 0.0, k: ease((ms - 260.0) / 800.0), ..rest },
        Anim::Rising if ms < 300.0 => Vals { mode: Mode::Map, amp: 1.0 - ease(ms / 300.0), full: Some(0.0), ..rest },
        Anim::Rising => {
            let p = ease((ms - 300.0) / 480.0);
            Vals { mode: Mode::Dot, mv: 1.0 - p, dot_r: s.pen * (1.0 - p), full: Some(p), ..rest }
        }
    }
}

#[derive(Debug, Clone)]
pub enum DeckMsg {
    Seek(u64),
    Play,
    Export(String),
    ShotMenu,
    ShotWindows(Vec<WindowInfo>),
}

/// What a deck shows.
#[derive(Clone, Copy)]
pub struct DeckCtx<'a> {
    pub app: &'a App,
    pub tr: &'a Transcript,
    pub anim: &'a DeckAnim,
    pub compact: bool,
    pub state: DeckState,
    pub player: bool,
    pub playing: bool,
    pub playing_ms: Option<u64>,
    /// Shows 「新しい録音」 (the main window only).
    pub on_new: bool,
    pub wrap: fn(DeckMsg) -> Message,
}

impl DeckCtx<'_> {
    fn sizes(&self) -> &'static Sizes {
        if self.compact { &COMPACT } else { &EXPANDED }
    }

    /// Both sides are recorded: until the session is loaded, from the recording, or before it
    /// from the picker.
    fn two(&self) -> bool {
        let ids: Vec<SourceId> = if !self.tr.sources.is_empty() {
            self.tr.sources.iter().map(|s| s.id).collect()
        } else if let Some(r) = &self.app.rec.sources {
            r.ids.clone()
        } else {
            let s = &self.app.source;
            match s.mode {
                SourceMode::Mic => vec![SourceId::Mic],
                m => {
                    let mut v = vec![if m == SourceMode::App { SourceId::App } else { SourceId::System }];
                    if s.include_mic {
                        v.push(SourceId::Mic);
                    }
                    v
                }
            }
        };
        ids.contains(&SourceId::Mic) && ids.iter().any(|i| *i != SourceId::Mic)
    }

    /// The session's length on its line: its duration, or its last line's end.
    fn total(&self) -> u64 {
        let mut total = self.tr.duration_ms.unwrap_or(0);
        for i in &self.tr.items {
            if let TimelineItem::Segment(s) = i {
                total = total.max(s.t_end_ms);
            }
        }
        total
    }

    /// The deck's height: fixed while recording and after, the free space before (`None`), and
    /// in between while it eases.
    pub fn height(&self) -> Option<f32> {
        let s = self.sizes();
        let v = vals(self.anim, self.state, s, self.app.now);
        if !s.centred {
            return Some(s.height);
        }
        match (v.full, self.state) {
            (Some(f), _) => {
                let full = self.anim.full_h.get();
                let full = if full > s.height { full } else { crate::app::EXPANDED.height - 240.0 };
                Some(s.height + (full - s.height) * f)
            }
            (None, DeckState::Start) => None,
            (None, _) => Some(s.height),
        }
    }
}

fn stroke(color: Color, width: f32) -> Stroke<'static> {
    Stroke::default().with_color(color).with_width(width).with_line_cap(LineCap::Round).with_line_join(LineJoin::Round)
}

/// Everything the line is drawn from, taken out of the app so it can be drawn when GPUI paints.
struct LineData {
    s: &'static Sizes,
    v: Vals,
    state: DeckState,
    shape: Option<Rc<Shape>>,
    two: bool,
    total: u64,
    samples: Vec<crate::model::line::Sample>,
    /// How far the pen is between steps, and the steps still to write while finishing.
    frac: f32,
    pending: usize,
    playing_ms: Option<u64>,
    /// The pointer's place on the line and the time there.
    scrub: Option<(f32, String)>,
}

fn draw_line(frame: &mut Frame, d: &LineData) {
    let s = d.s;
    let v = d.v;
    let (w, h) = (frame.width(), frame.height());
    let pal = p();
    // Before a recording the expanded window holds 開始 in the middle of the free space, with
    // no line yet; recording, the line lies near the top of the deck.
    let rest_base = if s.centred { (h - START_FOOT) * START_LIFT } else { s.start_base };
    let base = if v.mode == Mode::Dot { rest_base + (s.base - rest_base) * v.mv } else { s.base };
    let paused = d.state == DeckState::Paused;
    let tint = match d.state {
        DeckState::Start => alpha(pal.accent_tint, v.mv),
        DeckState::Paused => pal.surface_sunken,
        DeckState::Review => mix(hint(), pal.accent_tint, v.k),
        _ => pal.accent_tint,
    };
    let tint = if d.state == DeckState::Start || v.mode == Mode::Dot { alpha(tint, v.mv) } else { tint };
    frame.fill_rectangle(Point::new(0.0, base), Sz::new(w, h - base + 1.0), tint);
    let grey = pal.border_strong;
    let pen_color = if paused { pal.border_strong } else { pal.accent };

    if v.mode == Mode::Dot {
        let rest_x = if s.centred { w / 2.0 } else { s.dot_x };
        let x = rest_x + (s.start_x - rest_x) * v.mv;
        let rest_alpha = if s.centred { v.mv } else { 1.0 };
        frame.stroke(
            &Path::line(Point::new(x, base), Point::new(w, base)),
            stroke(alpha(grey, rest_alpha), s.stroke).with_line_cap(LineCap::Butt),
        );
        frame.stroke(
            &Path::line(Point::new(0.0, base), Point::new(x, base)),
            stroke(alpha(grey, v.mv), s.stroke).with_line_cap(LineCap::Butt),
        );
        if v.dot_r > 0.0 {
            frame.fill(&Path::circle(Point::new(x, base), v.dot_r), pen_color);
        }
        return;
    }

    let mut ax = s.start_x;
    if v.mode == Mode::Live {
        let g = Geometry { base, up: s.up, down: s.down, step: s.step, start_x: s.start_x, end_pad: s.end_pad };
        let l = live_line::live_line(&d.samples, d.frac, d.pending, w, g, d.two);
        ax = l.pen.x;
        // The zoom out: the live line shrinks towards the pen while the whole session grows into place.
        let sl = 1.0 - 0.92 * v.k;
        let fade = 1.0 - v.k;
        let map = |pt: Point| Point::new(ax + (pt.x - ax) * sl, pt.y);
        let path = |cmds: &[live_line::Cmd]| Path::new(|b| build(b, cmds, map));
        frame.fill(&path(&l.fill), alpha(tint, fade));
        // Grey where nothing is written: before the pen started, and ahead of the line.
        frame.stroke(
            &Path::line(map(Point::new(0.0, base)), map(Point::new(l.x0, base))),
            stroke(alpha(grey, fade), s.stroke).with_line_cap(LineCap::Butt),
        );
        frame.stroke(&path(&l.ahead), stroke(alpha(grey, fade), s.stroke).with_line_cap(LineCap::Butt));
        for (a, b) in &l.gaps {
            let dash = Stroke {
                line_dash: LineDash { segments: &[2.0, 3.0] },
                ..stroke(alpha(grey, fade), 1.5).with_line_cap(LineCap::Butt)
            };
            frame.stroke(&Path::line(map(Point::new(*a, base)), map(Point::new(*b, base))), dash);
        }
        frame.stroke(&path(&l.m), stroke(alpha(pal.ink, fade), s.stroke));
        frame.stroke(&path(&l.o), stroke(alpha(pal.accent, fade), s.stroke));
        frame.stroke(&path(&l.pending), stroke(alpha(grey, fade), s.stroke));
        for x in &l.ticks {
            frame.fill(&Path::circle(map(Point::new(*x, base)), s.shot_r), alpha(pal.ink, fade));
        }
        if v.pen_r > 0.0 {
            frame.fill(&Path::circle(l.pen, v.pen_r), pen_color);
        }
    }
    if let Some(shape) = &d.shape
        && (v.k > 0.0 || v.mode == Mode::Map)
    {
        let a = s.map_amp * v.amp;
        let l = map_line(shape, w, MapGeometry { pad: 0.0, base, up: s.up * a, down: s.down * a }, d.two);
        let sm = 5.0 - 4.0 * v.k;
        let x = |px: f32| ax + (px - ax) * sm;
        let pts = |v: &[Point]| v.iter().map(|q| Point::new(x(q.x), q.y)).collect::<Vec<_>>();
        let (o, m) = (pts(&l.o), pts(&l.m));
        let k = v.k;
        if !o.is_empty() {
            let fill = Path::new(|b| {
                b.move_to(Point::new(x(l.x0), base + 1.0));
                crate::model::line::smooth(b, &o, true);
                b.line_to(Point::new(x(l.x1), base + 1.0));
                b.close();
            });
            frame.fill(&fill, alpha(tint, k));
        }
        if !m.is_empty() {
            frame.stroke(&Path::new(|b| crate::model::line::smooth(b, &m, false)), stroke(alpha(pal.ink, k), s.stroke));
        }
        frame.stroke(&Path::new(|b| crate::model::line::smooth(b, &o, false)), stroke(alpha(pal.accent, k), s.stroke));
        for t in &l.ticks {
            frame.fill(&Path::circle(Point::new(x(*t), base), s.shot_r), alpha(pal.ink, k));
        }
    }
    // Where the sound being played is, on the session's line (FR-09).
    if d.state == DeckState::Review
        && let Some(ms) = d.playing_ms
        && d.total > 0
    {
        let px = (ms as f32 / d.total as f32).min(1.0) * w;
        frame.fill_rectangle(Point::new(px - 1.0, base - 16.0), Sz::new(2.0, 28.0), pal.ink);
    }
    // Pointing at a finished session's line: when that was.
    if let Some((at, label)) = &d.scrub {
        let px = at * w;
        frame.fill_rectangle(Point::new(px, base - 22.0), Sz::new(1.0, 40.0), pal.ink);
        let size = tokens::CAPTION.size - 1.0;
        let tw = label.len() as f32 * size * 0.62 + 8.0;
        let lx = (px - tw / 2.0).clamp(0.0, (w - tw).max(0.0));
        frame.fill_rectangle(Point::new(lx, base - 22.0 - 16.0), Sz::new(tw, 15.0), pal.surface);
        frame.fill_text(paint::Text {
            content: label.clone(),
            position: Point::new(lx + tw / 2.0, base - 22.0 - 8.5),
            color: pal.ink,
            size,
            font: theme::MONO,
        });
    }
}

/// The line, drawn across the deck; a finished session's can be pointed at and clicked to jump.
fn line_canvas(c: DeckCtx<'_>, v: Vals, shape: Option<Rc<Shape>>, total: u64) -> El {
    let app = c.app;
    let recording = c.state == DeckState::Recording;
    let frac = match app.line.at {
        Some(at) if recording => (app.now.saturating_duration_since(at).as_secs_f32() * 10.0).clamp(0.0, 1.0),
        _ => 0.0,
    };
    let pending = if c.state == DeckState::Finishing {
        let left = match &app.rec.finishing {
            None => 1.0,
            Some(f) if f.total > 0 => (f.total.saturating_sub(f.done)) as f32 / f.total as f32,
            Some(_) => 0.0,
        };
        (c.anim.pending_from.get() as f32 * left).round() as usize
    } else {
        0
    };
    let seekable = c.state == DeckState::Review && total > 0;
    let scrub = if seekable { c.anim.scrub.get() } else { None };
    let scrub = scrub.map(|at| {
        let ms = (at.clamp(0.0, 1.0) * total as f32) as u64;
        (at, if c.tr.started_at.is_some() { c.tr.clock(ms) } else { timer_text(ms) })
    });
    let live = v.mode == Mode::Live;
    let data = LineData {
        s: c.sizes(),
        v,
        state: c.state,
        shape,
        two: c.two(),
        total,
        samples: if live { app.line.samples.clone() } else { Vec::new() },
        frac,
        pending,
        playing_ms: c.playing_ms,
        scrub: scrub.clone(),
    };
    let bounds = c.anim.bounds.clone();
    let canvas = canvas_el(Some(bounds.clone()), move |frame| draw_line(frame, &data)).size_full();
    let mut area = div().id(uid("deck-line")).absolute().inset_0().child(canvas);
    if seekable {
        let base = c.sizes().base;
        let cell = c.anim.scrub.clone();
        let b2 = bounds.clone();
        let wrap = c.wrap;
        let at = move |pos: gpui_kit::Point<Pixels>| -> Option<f32> {
            let b = b2.get();
            let x = f32::from(pos.x - b.origin.x);
            let y = f32::from(pos.y - b.origin.y);
            let w = f32::from(b.size.width);
            (w > 0.0 && y >= base - 20.0 && y <= base + 18.0).then(|| (x / w).clamp(0.0, 1.0))
        };
        let at2 = at.clone();
        let cell2 = cell.clone();
        area = area
            .on_mouse_move(move |e, window, _| {
                let now = at(e.position);
                if cell.get() != now {
                    cell.set(now);
                    window.refresh();
                }
            })
            .on_hover(move |hovered, window, _| {
                if !*hovered && cell2.get().is_some() {
                    cell2.set(None);
                    window.refresh();
                }
            })
            .on_mouse_down(MouseButton::Left, move |e, _, cx| {
                if let Some(x) = at2(e.position) {
                    let ms = (x * total as f32) as u64;
                    crate::ui::send(wrap(DeckMsg::Seek(ms)), cx);
                }
            });
        if scrub.is_some() {
            area = area.cursor_pointer();
        }
    }
    area.into_any_element()
}

/// A filled shape the icons draw only in outline: the stop square, the pause bars.
fn block(w: f32, h: f32, color: Color) -> El {
    div().flex_none().w(px(w)).h(px(h)).rounded(px(1.5)).bg(color).into_any_element()
}

/// A control in the deck's row: square, on the raised ground, edged; `on` inverts it like a
/// pressed button (the paused pause, the playing play).
fn ctl(glyph: Icon, label: String, side: f32, on: bool, msg: Option<Message>) -> El {
    let fg = if on { p().accent_deep } else { p().ink };
    let fg = if msg.is_some() { fg } else { alpha(fg, 0.4) };
    ctl_with(icon(glyph, 16.0, fg).into_any_element(), label, side, on, msg)
}

/// The box of a deck control or of a button in the finished session's row.
fn boxed(msg: Option<Message>, on: bool, seed: &str) -> gpui_kit::Stateful<gpui_kit::Div> {
    let p = p();
    let sunk = p.surface_sunken;
    let b = clickable(msg, seed)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .border_1()
        .rounded(px(tokens::RADIUS_MD))
        .text_color(if on { p.accent_deep } else { p.ink });
    if on {
        b.bg(p.accent_tint).border_color(p.accent_tint)
    } else {
        b.bg(p.surface_raised).border_color(p.border_strong).hover(move |s| s.bg(sunk))
    }
}

fn ctl_with(content: El, label: String, side: f32, on: bool, msg: Option<Message>) -> El {
    let b = boxed(msg, on, &label).size(px(side)).child(content);
    with_tip(b, label).into_any_element()
}

/// Two controls joined into one (the camera and its menu, コピー and its formats): one edge
/// around both, a hairline between.
fn split(main: El, menu: El, height: f32) -> El {
    row()
        .flex_none()
        .h(px(height))
        .p(px(1.0))
        .items_stretch()
        .bg(p().surface_raised)
        .border_1()
        .border_color(p().border_strong)
        .rounded(px(tokens::RADIUS_MD))
        .child(main)
        .child(div().w(px(1.0)).h_full().bg(p().border_strong))
        .child(menu)
        .into_any_element()
}

/// A plain part of a joined control.
fn part(content: El, width: Option<f32>, msg: Option<Message>, tip: String, pad: f32) -> El {
    let sunk = p().surface_sunken;
    let mut b = clickable(msg, ("part", &tip))
        .flex()
        .items_center()
        .justify_center()
        .h_full()
        .px(px(pad))
        .rounded(px(tokens::RADIUS_MD - 1.0))
        .text_color(p().ink)
        .hover(move |s| s.bg(sunk))
        .child(content);
    if let Some(w) = width {
        b = b.w(px(w));
    }
    with_tip(b, tip).into_any_element()
}

/// The row under the line: at the bottom of the deck, the line's start and end in line with it.
fn row_layer(content: El, compact: bool) -> El {
    let (x, bottom, h) = if compact { (12.0, 7.0, 38.0) } else { (14.0, 12.0, 44.0) };
    div()
        .absolute()
        .left(px(x))
        .right(px(x))
        .bottom(px(bottom))
        .h(px(h))
        .flex()
        .items_center()
        .child(content)
        .into_any_element()
}

/// The deck.
pub fn view(c: DeckCtx<'_>) -> El {
    let s = c.sizes();
    let v = vals(c.anim, c.state, s, c.app.now);
    let shape = matches!(c.state, DeckState::Review).then(|| c.anim.shape(c.tr));
    let total = c.total();
    if c.state == DeckState::Start && !c.compact {
        // The start deck's free space, remembered so the deck can ease to and from it.
        let h = f32::from(c.anim.bounds.get().size.height);
        if h > 0.0 {
            c.anim.full_h.set(h);
        }
    }
    let moving = c.anim.moving(c.app.now);
    let mut deck = div().relative().w_full().child(line_canvas(c, v, shape, total));
    match c.state {
        DeckState::Start if !moving || !matches!(c.anim.anim.get(), Some((Anim::Starting, _))) => {
            if !moving {
                deck = deck.child(start_layer(c));
            }
        }
        DeckState::Start => {}
        DeckState::Recording | DeckState::Paused => {
            if !moving {
                if let Some(note) = line_note(c) {
                    deck = deck.child(note);
                }
                deck = deck.child(row_layer(live_row(c), c.compact));
            }
        }
        DeckState::Finishing => deck = deck.child(row_layer(finishing_row(c), c.compact)),
        DeckState::Review => {
            if !c.compact && total > 0 {
                deck = deck.child(line_ends(c, total));
            }
            if !moving {
                deck = deck.child(row_layer(review_row(c), c.compact));
            }
        }
    }
    match c.height() {
        Some(h) => deck.flex_none().h(px(h)),
        None => deck.flex_1().min_h_0(),
    }
    .into_any_element()
}

fn start_layer(c: DeckCtx<'_>) -> El {
    let app = c.app;
    if c.compact {
        let mut layer = div().absolute().inset_0().child(
            div()
                .absolute()
                .left(px(COMPACT.dot_x))
                .top(px(COMPACT.start_base - 18.0))
                .child(go_button(app, 36.0, 12.0)),
        );
        if let Some(hint) = hotkey_hint(app) {
            layer = layer.child(
                div()
                    .absolute()
                    .right(px(12.0))
                    .top(px(COMPACT.start_base - 5.0 - tokens::CAPTION.line_height))
                    .child(hint),
            );
        }
        return layer.into_any_element();
    }
    // 開始 rests START_LIFT of the way down the room above the foot, the hints under it.
    let mut under = col().items_center().gap(px(6.0));
    if let Some(h) = hotkey_hint(app) {
        under = under.child(h);
    }
    under = under.child(super::source_picker::file_link(app, false));
    let anchor = div()
        .relative()
        .flex_none()
        .w_full()
        .h(px(0.0))
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(-32.0))
                .flex()
                .justify_center()
                .child(go_button(app, 64.0, 20.0)),
        )
        .child(div().absolute().left_0().right_0().top(px(tokens::SPACE_12)).flex().justify_center().child(under));
    let mut layer = col()
        .absolute()
        .inset_0()
        .child(grow(START_LIFT))
        .child(anchor)
        .child(grow(1.0 - START_LIFT))
        .child(div().flex_none().h(px(START_FOOT)));
    if let Some(note) = crate::screens::models::note(app) {
        layer = layer.child(div().absolute().left_0().right_0().bottom(px(14.0)).flex().justify_center().child(note));
    }
    layer.into_any_element()
}

/// Empty space that takes `share` of what is left in a column.
fn grow(share: f32) -> gpui_kit::Div {
    let mut d = div().flex_basis(px(0.0)).min_h_0();
    d.style().flex_grow = Some(share);
    d
}

/// 「Ctrl + Shift + R でも開始できます」, unless no key is set or another app holds it.
fn hotkey_hint(app: &App) -> Option<El> {
    let key = app.settings.hotkeys.toggle.trim();
    if key.is_empty() || app.info.hotkey_errors.contains_key("toggle") {
        return None;
    }
    Some(line(tf("startHint", &[("key", &key.replace('+', " + "))]), tokens::CAPTION, p().ink_muted).into_any_element())
}

/// 開始, the one primary action before a recording: round, with the record dot (the only round
/// control, DECISIONS → UI), the kamon's inner line inside its edge.
fn go_button(app: &App, side: f32, dot: f32) -> El {
    let enabled = app.rec.state == UiState::Ready;
    let p = p();
    let dot_el = div().size(px(dot)).rounded(px(dot / 2.0)).bg(p.on_accent);
    let inner = div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(p.on_accent)
        .rounded(px(side / 2.0 - 3.0))
        .child(dot_el);
    let hover = p.accent_hover;
    let mut b = clickable(enabled.then_some(Message::Action(Action::ToggleRecording)), "go")
        .flex_none()
        .size(px(side))
        .p(px(3.0))
        .rounded(px(side / 2.0))
        .bg(p.accent)
        .child(inner);
    if enabled {
        b = b.hover(move |s| s.bg(hover));
    } else {
        b = b.opacity(0.45);
    }
    with_tip(b, t("start")).into_any_element()
}

/// 遅れ n秒 / 一時停止中: written on the line's oldest end, on the ground, since the row below has
/// no room for it. Small and grey while it may catch up, bold from 15 s, ochre from 30 s (FR-25).
/// How loud the note on the line speaks: grey, bold, or bold in ochre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Quiet,
    Bold,
    Warn,
}

/// The note on the line, from the pause, the deferral and the lag in ms.
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

fn line_note(c: DeckCtx<'_>) -> Option<El> {
    let rec = &c.app.rec;
    let paused = c.state == DeckState::Paused;
    let (text, tone) = note_of(paused, rec.deferred, rec.lag.as_ref().map_or(0, |l| l.lag_ms))?;
    let (color, medium) = match tone {
        Tone::Quiet => (p().ink_muted, false),
        Tone::Bold => (p().ink, true),
        Tone::Warn => (p().warning, true),
    };
    let label = line(text, tokens::CAPTION, color);
    let mut r = row().px(px(4.0)).rounded(px(4.0)).bg(p().surface).child(if medium { label.medium() } else { label });
    if rec.deferred && !paused {
        r = r.child(line(" · ", tokens::CAPTION, p().ink_muted)).child(
            clickable(Some(Message::Action(Action::DeferTranscription(false))), "resume-transcription")
                .child(line(t("resumeTranscription"), tokens::CAPTION, p().accent_deep).medium()),
        );
    }
    let s = c.sizes();
    Some(div().absolute().left(px(8.0)).top(px(s.base - 9.0)).child(r).into_any_element())
}

/// The time in big numerals, then pause or resume, stop, screenshot (with what it takes), mark and cut.
fn live_row(c: DeckCtx<'_>) -> El {
    let app = c.app;
    let paused = c.state == DeckState::Paused;
    let side = if c.compact { 36.0 } else { 42.0 };
    let timer_style = if c.compact { tokens::HEADING } else { tokens::TITLE };
    let timer = with_tip(
        line(timer_text(app.rec.elapsed(app.now)), timer_style, if paused { p().ink_muted } else { p().ink }).mono(),
        t("elapsed"),
    );
    let pause = if paused {
        ctl(Icon::Play, t("resume"), side, true, Some(Message::Action(Action::PauseResume)))
    } else {
        let bars = row().gap(px(4.0)).child(block(4.0, 14.0, p().ink)).child(block(4.0, 14.0, p().ink));
        ctl_with(bars.into_any_element(), t("pause"), side, false, Some(Message::Action(Action::PauseResume)))
    };
    let stop = btn_with(
        row().gap(px(8.0)).child(block(10.0, 10.0, p().on_accent)).child(plain(t("stop"), tokens::LABEL)),
        Variant::Primary,
        Size::Lg,
        Some(Message::Action(Action::Stop)),
    )
    .h(px(side));
    let shot_tip = match &app.rec.shot_window {
        Some(w) => format!("{}: {}", t("screenshot"), w.title),
        None => t("screenshot"),
    };
    let camera = split(
        part(
            icon(Icon::Camera, 17.0, if paused { alpha(p().ink, 0.4) } else { p().ink }).into_any_element(),
            Some(side - 2.0),
            (!paused).then_some(Message::Action(Action::Screenshot)),
            shot_tip,
            0.0,
        ),
        part(
            icon(Icon::ChevronDown, 12.0, p().ink_muted).into_any_element(),
            Some(if c.compact { 16.0 } else { 18.0 }),
            Some((c.wrap)(DeckMsg::ShotMenu)),
            t("shotTarget"),
            0.0,
        ),
        side,
    );
    let controls = row()
        .flex_none()
        .gap(px(8.0))
        .child(pause)
        .child(stop)
        .child(camera)
        .child(ctl(Icon::Star, t("markLine"), side, false, Some(Message::Action(Action::MarkLine))))
        .child(ctl(Icon::Scissors, t("cut"), side, false, Some(Message::Action(Action::Cut))));
    row()
        .w_full()
        .gap(px(12.0))
        .child(div().flex_1().min_w_0().overflow_hidden().child(timer))
        .child(controls)
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

fn finishing_row(c: DeckCtx<'_>) -> El {
    let app = c.app;
    let f = app.rec.finishing.as_ref();
    // A file (FR-18) shows how far into it the text has got, from the start.
    let from_file = app.rec.sources.as_ref().is_some_and(|s| s.ids.contains(&SourceId::File));
    let mut words = row()
        .min_w_0()
        .overflow_hidden()
        .child(line(t(if from_file { "transcribingFile" } else { "finishing" }), tokens::CAPTION, p().ink).medium());
    if let Some(f) = f.filter(|f| from_file && f.total_ms > 0) {
        words = words.child(
            line(format!(" · {} / {}", timer_text(f.done_ms), timer_text(f.total_ms)), tokens::CAPTION, p().ink_muted)
                .mono(),
        );
    }
    if let Some(f) = f.filter(|f| f.total > 0) {
        words = words.child(line(
            format!(" · {}", tf("finishingLeft", &[("n", &(f.total - f.done))])),
            tokens::CAPTION,
            p().ink_muted,
        ));
    }
    // The spinner says it is still working while one long stretch keeps the count still.
    let mut left = col().gap(px(6.0)).child(row().gap(px(6.0)).child(spinner(12.0, app.now)).child(words));
    let long = c.anim.finishing_since.get().is_some_and(|at| app.now.saturating_duration_since(at) >= FINISHING_BAR);
    // By audio rather than by line: the lines left differ in length, merged ones most.
    if let Some(f) = f.filter(|f| (long || from_file) && f.total_ms > 0) {
        left = left.child(div().max_w(px(260.0)).child(progress(f.done_ms as f32, f.total_ms as f32)));
    }
    let left: El =
        if from_file { left.into_any_element() } else { with_tip(left, t("finishingDetail")).into_any_element() };
    let ink = p().ink;
    let stop = clickable(Some(Message::Action(Action::CancelFinishing)), "stop-finishing")
        .px(px(4.0))
        .py(px(2.0))
        .text_color(p().ink_muted)
        .hover(move |s| s.text_color(ink))
        .child(theme::plain(t("stopFinishing"), tokens::CAPTION).medium());
    row().w_full().gap(px(12.0)).child(div().flex_1().min_w_0().child(left)).child(stop).into_any_element()
}

/// A label in the finished session's row, a step under the kit's label so the row fits the window.
fn act_label(s: String) -> gpui_kit::Div {
    plain(s, tokens::LABEL).text_size(px(tokens::LABEL.size - 1.0))
}

/// A button in the finished session's row.
fn act(content: impl IntoElement, side: f32, square: bool, on: bool, msg: Message, tip: String) -> El {
    let mut b = boxed(Some(msg), on, &tip).h(px(side)).child(row().gap(px(6.0)).child(content));
    b = if square { b.w(px(side)) } else { b.px(px(10.0)) };
    with_tip(b, tip).into_any_element()
}

/// 再生 (when the session kept its sound), コピー ▾, 書き出し, ⋯ and 「新しい録音」.
fn review_row(c: DeckCtx<'_>) -> El {
    let app = c.app;
    let Some(id) = c.tr.session_id.clone() else { return nothing() };
    let side = if c.compact { 36.0 } else { 40.0 };
    let mut r = row().w_full().gap(px(6.0));
    if c.player {
        let (glyph, tip) = if c.playing { (Icon::Pause, t("pausePlayback")) } else { (Icon::Play, t("play")) };
        let tone = if c.playing { p().accent_deep } else { p().ink };
        r = r.child(act(icon(glyph, 15.0, tone), side, true, c.playing, (c.wrap)(DeckMsg::Play), tip));
    }
    // コピー copies plain text; its chevron picks the other formats.
    let copy_menu = vec![
        Item::action(format!("action:copy-agent:{id}"), t("copyForAgent")),
        Item::action(format!("action:copy-markdown:{id}"), t("copyMarkdown")),
    ];
    r = r.child(split(
        part(
            row().gap(px(6.0)).child(icon(Icon::Copy, 15.0, p().ink)).child(act_label(t("copy"))).into_any_element(),
            None,
            Some(Message::Action(Action::Copy(CopyFormat::Plain, Some(id.clone())))),
            String::new(),
            10.0,
        ),
        part(
            icon(Icon::ChevronDown, 14.0, p().ink_muted).into_any_element(),
            Some(20.0),
            Some(Message::Popup(copy_menu, None)),
            t("copyAs"),
            0.0,
        ),
        side,
    ));
    if c.compact {
        // 書き出し picks a format in a dialog; the compact window is too small for one.
        let menu = vec![
            Item::action(format!("action:export-markdown:{id}"), t("exportMarkdown")),
            Item::action(format!("action:export-zip:{id}"), t("exportMarkdownZip")),
            Item::action(format!("action:export-pdf:{id}"), t("exportPdf")),
            Item::action(format!("action:export-typst:{id}"), t("exportTypst")),
        ];
        r = r.child(act(
            icon(Icon::FileOutput, 16.0, p().ink),
            side,
            true,
            false,
            Message::Popup(menu, None),
            t("export"),
        ));
    } else {
        r = r.child(act(
            row().gap(px(6.0)).child(icon(Icon::FileOutput, 15.0, p().ink)).child(act_label(t("export"))),
            side,
            false,
            false,
            (c.wrap)(DeckMsg::Export(id.clone())),
            String::new(),
        ));
    }
    let more =
        session_menu::items(app, SessionRef { id: &id, recoverable: false, project: c.tr.project.as_deref() }, false);
    r = r.child(act(
        icon(Icon::Ellipsis, 17.0, p().ink),
        side,
        true,
        false,
        Message::Popup(more, None),
        t("moreActions"),
    ));
    r = r.child(fill());
    if c.on_new {
        r = r.child(act(
            act_label(t("newRecording")),
            side,
            false,
            false,
            Message::Action(Action::NewRecording),
            String::new(),
        ));
    }
    r.into_any_element()
}

/// The finished session's ends under its line: its clock times, or for a file its start and
/// length (FR-18), since hours and minutes would read as minutes and seconds.
fn line_ends(c: DeckCtx<'_>, total: u64) -> El {
    let (a, b) = if c.tr.is_from_file() || c.tr.started_at.is_none() {
        (timer_text(0), timer_text(total))
    } else {
        (c.tr.clock(0).chars().take(5).collect(), c.tr.clock(total).chars().take(5).collect())
    };
    let style = |s: String| line(s, tokens::CAPTION, p().ink_muted).mono().text_size(px(tokens::CAPTION.size - 1.0));
    row()
        .absolute()
        .left(px(14.0))
        .right(px(14.0))
        .top(px(EXPANDED.base + 7.0))
        .child(style(a))
        .child(fill())
        .child(style(b))
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
    fn moves_start_only_for_a_change_seen_on_screen() {
        let a = DeckAnim::default();
        a.observe(DeckState::Recording, 0);
        assert!(a.anim.get().is_none());
        a.observe(DeckState::Review, 0);
        assert_eq!(a.anim.get().map(|x| x.0), Some(Anim::Settling));
        a.forget();
        a.observe(DeckState::Start, 0);
        assert!(a.anim.get().is_none());
        a.observe(DeckState::Recording, 0);
        assert_eq!(a.anim.get().map(|x| x.0), Some(Anim::Starting));
        a.observe(DeckState::Finishing, 120);
        assert_eq!(a.pending_from.get(), 40);
    }

    #[test]
    fn a_move_eases_from_where_it_was_to_where_it_goes() {
        let a = DeckAnim::default();
        a.observe(DeckState::Start, 0);
        a.observe(DeckState::Recording, 0);
        let at = a.anim.get().expect("a move").1;
        let begin = vals(&a, DeckState::Recording, &EXPANDED, at);
        assert_eq!((begin.mode, begin.mv, begin.full), (Mode::Dot, 0.0, Some(1.0)));
        let end = vals(&a, DeckState::Recording, &EXPANDED, at + Duration::from_millis(500));
        assert_eq!((end.mode, end.pen_r, end.full), (Mode::Live, EXPANDED.pen, None));
    }

    #[test]
    fn native_menus_show_ampersands() {
        assert_eq!(menu_text("Q&A"), "Q&&A");
    }
}
