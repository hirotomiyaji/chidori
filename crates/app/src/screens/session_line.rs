//! A whole session's line (`mapLine` in the React UI): its shape across a width, 相手 above the
//! baseline in the accent, 自分 below it in ink, screenshots as dots on the baseline. Small and
//! still in 履歴's rows; the deck draws the same shape larger for a finished session.

use gpui_kit::{IntoElement, Styled, px};

use crate::model::line::{Shape, smooth};
use crate::paint::{Frame, LineCap, LineJoin, Path, Point, Stroke, canvas_el};
use crate::theme::{Color, p};
use crate::ui::El;

/// Where a session's line lies in a box: its ends, baseline and how far each side reaches.
#[derive(Debug, Clone, Copy)]
pub struct MapGeometry {
    pub pad: f32,
    pub base: f32,
    pub up: f32,
    pub down: f32,
}

/// The points of each side's line and the screenshots' x positions.
pub struct MapLine {
    pub o: Vec<Point>,
    pub m: Vec<Point>,
    pub ticks: Vec<f32>,
    pub x0: f32,
    pub x1: f32,
}

/// Lays a shape across `width`. With one source its line rises above the baseline whichever
/// side it is; a 1-2-1 blur keeps a lone slice from looking like a spike.
pub fn map_line(shape: &Shape, width: f32, g: MapGeometry, two: bool) -> MapLine {
    let n = shape.o.len().max(shape.m.len()).max(2);
    let x = |i: usize| g.pad + (i as f32 / (n - 1) as f32) * (width - 2.0 * g.pad);
    let at = |a: &[f32], i: usize| a.get(i).copied().unwrap_or(0.0);
    let top = |i: usize| if two { at(&shape.o, i) } else { at(&shape.o, i).max(at(&shape.m, i)) };
    let blur = |f: &dyn Fn(usize) -> f32, i: usize| (f(i.saturating_sub(1)) + 2.0 * f(i) + f((i + 1).min(n - 1))) / 4.0;
    let o = (0..n).map(|i| Point::new(x(i), g.base - blur(&top, i) * g.up)).collect();
    let me = |i: usize| at(&shape.m, i);
    let m = if two { (0..n).map(|i| Point::new(x(i), g.base + blur(&me, i) * g.down)).collect() } else { Vec::new() };
    MapLine { o, m, ticks: shape.shots.iter().map(|s| x(*s)).collect(), x0: x(0), x1: x(n - 1) }
}

/// Draws a session's line into a frame: the baseline, the sides, the screenshot dots. `fill`, when
/// set, is the tint under 相手's line.
pub fn draw(frame: &mut Frame, l: &MapLine, base: f32, width: f32, fill: Option<Color>, rest: Color) {
    let stroke = |c: Color, w: f32| {
        Stroke::default().with_color(c).with_width(w).with_line_cap(LineCap::Round).with_line_join(LineJoin::Round)
    };
    if let Some(fill) = fill
        && !l.o.is_empty()
    {
        let under = Path::new(|b| {
            b.move_to(Point::new(l.x0, base + 1.0));
            smooth(b, &l.o, true);
            b.line_to(Point::new(l.x1, base + 1.0));
            b.close();
        });
        frame.fill(&under, fill);
    }
    frame.stroke(&Path::line(Point::new(l.x0, base), Point::new(l.x1, base)), stroke(rest, 1.0));
    if !l.m.is_empty() {
        frame.stroke(&Path::new(|b| smooth(b, &l.m, false)), stroke(p().ink, width));
    }
    frame.stroke(&Path::new(|b| smooth(b, &l.o, false)), stroke(p().accent, width));
    for x in &l.ticks {
        frame.fill(&Path::circle(Point::new(*x, base), 2.25), p().ink);
    }
}

const H: f32 = 30.0;
const BASE: f32 = 15.0;

/// A past session's line, still and small (履歴's rows): 30 px high, the full width.
pub fn small(shape: &Shape, two: bool) -> El {
    let shape = shape.clone();
    canvas_el(None, move |frame| {
        let l = map_line(&shape, frame.width(), MapGeometry { pad: 2.0, base: BASE, up: 10.0, down: 8.0 }, two);
        draw(frame, &l, BASE, 1.8, None, p().border);
    })
    .w_full()
    .h(px(H))
    .into_any_element()
}
