//! The live line's geometry (DECISIONS → UI, "The line is real sound"): 相手 above the baseline,
//! 自分 below it, the tint under it up to its top edge, the grey line where nothing is written,
//! pauses as gaps and screenshots as dots. Pure geometry, as path commands, so the deck can squeeze
//! it towards the pen when it zooms out to the whole session.

use crate::model::line::Sample;
use crate::paint::{Builder, Point};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    Move(Point),
    Line(Point),
    Curve(Point, Point, Point),
    Close,
}

pub type Path = Vec<Cmd>;

/// A smooth path through the points (Catmull-Rom as cubic Béziers).
pub fn smooth(pts: &[Point]) -> Path {
    let Some(first) = pts.first() else { return Vec::new() };
    let mut d = vec![Cmd::Move(*first)];
    for i in 0..pts.len() - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = *pts.get(i + 2).unwrap_or(&p2);
        d.push(Cmd::Curve(
            Point::new(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0),
            Point::new(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0),
            p2,
        ));
    }
    d
}

/// Appends commands to a canvas path, each point mapped first (the deck's zoom).
pub fn build(b: &mut Builder, cmds: &[Cmd], map: impl Fn(Point) -> Point) {
    for c in cmds {
        match *c {
            Cmd::Move(p) => b.move_to(map(p)),
            Cmd::Line(p) => b.line_to(map(p)),
            Cmd::Curve(a, c2, p) => b.bezier_curve_to(map(a), map(c2), map(p)),
            Cmd::Close => b.close(),
        }
    }
}

/// Sizes in px: the baseline's y, how far each side reaches, one step's width, where the pen starts
/// and how far from the end it stops.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    pub base: f32,
    pub up: f32,
    pub down: f32,
    pub step: f32,
    pub start_x: f32,
    pub end_pad: f32,
}

#[derive(Debug, Clone, Default)]
pub struct LiveLine {
    /// 相手's line, 自分's line, and the steps still being written (both sides).
    pub o: Path,
    pub m: Path,
    pub pending: Path,
    /// The tint under the line, up to its top edge: closed shapes ending just below the baseline.
    pub fill: Path,
    /// The line not written yet: from the end of the line back down to the baseline, to the edge.
    pub ahead: Path,
    /// Pauses as (from, to) on the baseline, screenshots as x positions.
    pub gaps: Vec<(f32, f32)>,
    pub ticks: Vec<f32>,
    /// Where the visible line starts, and the pen tip.
    pub x0: f32,
    pub pen: Point,
}

/// How far the line not written yet takes to come back down from the end of the line.
const SLOPE: f32 = 12.0;

/// The area under a smooth path, closed 1 px below the baseline so it overlaps the tint below.
fn under(curve: &[Cmd], xa: f32, xb: f32, base: f32, tail: &[Cmd]) -> Path {
    let Some(Cmd::Move(first)) = curve.first() else { return Vec::new() };
    let mut d = vec![Cmd::Move(Point::new(xa, base + 1.0)), Cmd::Line(*first)];
    d.extend_from_slice(&curve[1..]);
    d.extend_from_slice(tail);
    d.push(Cmd::Line(Point::new(xb, base + 1.0)));
    d.push(Cmd::Close);
    d
}

/// From the end of the line at `y` back down to the baseline over `SLOPE` px.
fn slope_down(at: Point, base: f32) -> Option<Cmd> {
    if base - at.y < 0.5 {
        return None;
    }
    let h = SLOPE / 2.0;
    Some(Cmd::Curve(Point::new(at.x + h, at.y), Point::new(at.x + h, base), Point::new(at.x + SLOPE, base)))
}

/// The live line in a box `width` wide. The pen moves right from `start_x` until it is `end_pad`
/// from the end; after that the line scrolls under it. `frac` (0..1) is how far the pen has moved
/// past the newest step; the last `pending` steps are still being written. With one source its
/// line rises above the baseline whichever side it is.
pub fn live_line(samples: &[Sample], frac: f32, pending: usize, width: f32, g: Geometry, two: bool) -> LiveLine {
    let n = samples.len();
    let t = (n as f32 - 1.0 + frac).max(0.0);
    let px = (width - g.end_pad).min(g.start_x + t * g.step);
    let xi = |i: usize| px - (t - i as f32) * g.step;
    let lo = (t - (px + 4.0 * g.step) / g.step).floor().max(0.0) as usize;
    let written = n - pending.min(n);
    let up = |s: &Sample| if two { s.o } else { s.o.max(s.m) };
    // A light 1-2-1 blur, so a step that stands alone doesn't look like a spike.
    let soft = |i: usize, f: &dyn Fn(&Sample) -> f32| {
        let v = f(&samples[i]);
        let a = i.checked_sub(1).map(|k| &samples[k]).filter(|s| !s.gap).map_or(v, f);
        let c = samples.get(i + 1).filter(|s| !s.gap).map_or(v, f);
        (a + 2.0 * v + c) / 4.0
    };

    let mut runs: Vec<Vec<usize>> = vec![Vec::new()];
    let mut gaps = Vec::new();
    let mut ticks = Vec::new();
    let mut gap_from: Option<f32> = None;
    for (i, s) in samples.iter().enumerate().skip(lo) {
        if s.gap {
            if gap_from.is_none() {
                gap_from = Some(xi(i));
                runs.push(Vec::new());
            }
            continue;
        }
        if let Some(from) = gap_from.take() {
            gaps.push((from, xi(i)));
        }
        runs.last_mut().expect("one run at least").push(i);
        if s.shot {
            ticks.push(xi(i));
        }
    }
    if let Some(from) = gap_from {
        gaps.push((from, px));
    }

    let yo = |i: usize| g.base - soft(i, &up) * g.up;
    let ym = |i: usize| g.base + soft(i, &|s: &Sample| s.m) * g.down;
    let last = samples.last();
    // The line ends at the newest step, or past it at the pen while it moves; what is not written
    // yet comes back down to the baseline from there.
    let end = match last {
        Some(s) if !s.gap => Point::new(px, yo(n - 1)),
        _ => Point::new(px, g.base),
    };
    let slope = slope_down(end, g.base);
    let slope_end = end.x + if slope.is_some() { SLOPE } else { 0.0 };

    let mut line = LiveLine::default();
    for run in runs.iter().filter(|r| !r.is_empty()) {
        let first = run[0];
        let fin = run[run.len() - 1];
        // A stretch rises from the baseline where it starts (the start, or the end of a pause) and
        // goes back down to it before a pause, so the line and its tint never end in mid-air.
        let head = (first == 0 || samples[first - 1].gap).then(|| Point::new(xi(first) - g.step, g.base));
        let foot = samples.get(fin + 1).filter(|s| s.gap).map(|_| Point::new(xi(fin) + g.step, g.base));
        let path = |idx: &[usize], y: &dyn Fn(usize) -> f32| {
            let mut pts: Vec<Point> = Vec::with_capacity(idx.len() + 2);
            if let Some(h) = head
                && idx.first() == Some(&first)
            {
                pts.push(h);
            }
            pts.extend(idx.iter().map(|&i| Point::new(xi(i), y(i))));
            if let Some(f) = foot
                && idx.last() == Some(&fin)
            {
                pts.push(f);
            }
            smooth(&pts)
        };
        let done: Vec<usize> = run.iter().copied().filter(|&i| i < written).collect();
        let open: Vec<usize> = run.iter().copied().filter(|&i| i + 1 >= written).collect();
        if !done.is_empty() {
            line.o.extend(path(&done, &yo));
            if two {
                line.m.extend(path(&done, &ym));
            }
        }
        if written < n && open.len() > 1 {
            line.pending.extend(path(&open, &yo));
            if two {
                line.pending.extend(path(&open, &ym));
            }
        }
        // The tint rises to the top of the line, written or not, and down its slope at the end.
        let ends = fin == n - 1;
        let xa = head.map_or(xi(first), |h| h.x);
        let xb = if ends { slope_end } else { foot.map_or(xi(fin), |f| f.x) };
        let mut tail = Vec::new();
        if ends {
            if frac > 0.0 {
                tail.push(Cmd::Line(end));
            }
            tail.extend(slope);
        }
        line.fill.extend(under(&path(run, &yo), xa, xb, g.base, &tail));
    }

    line.pen = Point::new(px, g.base);
    match last {
        Some(s) if !s.gap && written == n => {
            // The pen is a little past the newest step; the newest heights are carried to it.
            let xl = xi(n - 1);
            line.pen = end;
            if frac > 0.0 {
                line.o.extend([Cmd::Move(Point::new(xl, yo(n - 1))), Cmd::Line(Point::new(px, yo(n - 1)))]);
                if two {
                    line.m.extend([Cmd::Move(Point::new(xl, ym(n - 1))), Cmd::Line(Point::new(px, ym(n - 1)))]);
                }
            }
        }
        _ if written < n && written > 0 => {
            // While finishing, the pen waits where the written part ends.
            let i = written - 1;
            line.pen = Point::new(xi(i), if samples[i].gap { g.base } else { yo(i) });
        }
        _ => {}
    }
    line.ahead.push(Cmd::Move(end));
    line.ahead.extend(slope);
    let y = if slope.is_some() { g.base } else { end.y };
    line.ahead.push(Cmd::Line(Point::new(width.max(slope_end), y)));
    line.gaps = gaps;
    line.ticks = ticks;
    line.x0 = if n > 0 { xi(lo).max(0.0) } else { px };
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: Geometry = Geometry { base: 30.0, up: 20.0, down: 15.0, step: 2.0, start_x: 48.0, end_pad: 40.0 };

    fn quiet(n: usize) -> Vec<Sample> {
        vec![Sample::default(); n]
    }

    fn loud(n: usize) -> Vec<Sample> {
        vec![Sample { o: 1.0, m: 1.0, ..Default::default() }; n]
    }

    fn gap(n: usize) -> Vec<Sample> {
        vec![Sample { gap: true, ..Default::default() }; n]
    }

    fn r1(v: f32) -> f32 {
        (v * 10.0).round() / 10.0
    }

    /// The end point of each command, rounded as the line is drawn.
    fn ends(d: &[Cmd]) -> Vec<(f32, f32)> {
        d.iter()
            .filter_map(|c| match c {
                Cmd::Move(p) | Cmd::Line(p) | Cmd::Curve(_, _, p) => Some((r1(p.x), r1(p.y))),
                Cmd::Close => None,
            })
            .collect()
    }

    fn ys(d: &[Cmd]) -> Vec<f32> {
        ends(d).into_iter().map(|p| p.1).collect()
    }

    fn min(v: Vec<f32>) -> f32 {
        v.into_iter().fold(f32::INFINITY, f32::min)
    }

    fn max(v: Vec<f32>) -> f32 {
        v.into_iter().fold(f32::NEG_INFINITY, f32::max)
    }

    #[test]
    fn smooth_passes_through_every_point() {
        let pts = [Point::new(0.0, 0.0), Point::new(10.0, 5.0), Point::new(20.0, 0.0)];
        assert_eq!(ends(&smooth(&pts)), vec![(0.0, 0.0), (10.0, 5.0), (20.0, 0.0)]);
        assert!(smooth(&[]).is_empty());
    }

    #[test]
    fn the_pen_moves_right_until_it_nears_the_end_then_the_line_scrolls() {
        assert_eq!(live_line(&[], 0.0, 0, 400.0, G, true).pen, Point::new(48.0, 30.0));
        assert_eq!(live_line(&quiet(11), 0.0, 0, 400.0, G, true).pen.x, 68.0);
        let full = live_line(&quiet(1000), 0.5, 0, 400.0, G, true);
        assert_eq!(full.pen.x, 360.0);
        assert_eq!(full.x0, 0.0);
    }

    #[test]
    fn others_above_the_line_and_me_below_it() {
        let mut s = quiet(5);
        s.push(Sample { o: 1.0, ..Default::default() });
        s.extend(quiet(5));
        s.push(Sample { m: 1.0, ..Default::default() });
        s.extend(quiet(5));
        let l = live_line(&s, 0.0, 0, 400.0, G, true);
        assert!(min(ys(&l.o)) < 30.0);
        assert_eq!(max(ys(&l.o)), 30.0);
        assert!(max(ys(&l.m)) > 30.0);
    }

    #[test]
    fn a_single_source_rises_above_the_line_whichever_side_it_is() {
        let mut s = quiet(3);
        s.push(Sample { m: 1.0, ..Default::default() });
        s.extend(quiet(3));
        let l = live_line(&s, 0.0, 0, 400.0, G, false);
        assert!(l.m.is_empty());
        assert!(min(ys(&l.o)) < 30.0);
    }

    #[test]
    fn a_pause_leaves_a_gap_and_a_screenshot_a_dot() {
        let mut s = quiet(4);
        s.push(Sample { shot: true, ..Default::default() });
        s.extend(quiet(2));
        s.extend(gap(3));
        s.extend(quiet(4));
        let l = live_line(&s, 0.0, 0, 400.0, G, true);
        assert_eq!(l.ticks, vec![56.0]);
        assert_eq!(l.gaps, vec![(62.0, 68.0)]);
        assert_eq!(l.o.iter().filter(|c| matches!(c, Cmd::Move(_))).count(), 2);
    }

    #[test]
    fn steps_still_being_written_are_apart_with_the_pen_where_the_written_part_ends() {
        let l = live_line(&quiet(20), 0.0, 5, 400.0, G, true);
        assert!(!l.pending.is_empty());
        assert_eq!(l.pen.x, 48.0 + 14.0 * 2.0);
    }

    #[test]
    fn a_stretch_rises_from_the_baseline_and_goes_back_down_before_a_pause() {
        let mut s = loud(5);
        s.extend(gap(3));
        s.extend(loud(5));
        let l = live_line(&s, 0.0, 0, 400.0, G, true);
        let split = l.o.iter().skip(1).position(|c| matches!(c, Cmd::Move(_))).expect("two stretches") + 1;
        let (before, after) = (ends(&l.o[..split]), ends(&l.o[split..]));
        // Steps at 48, 50 … 56, the pause from 58, steps again from 64.
        assert_eq!(before[0], (46.0, 30.0));
        assert_eq!(*before.last().expect("points"), (58.0, 30.0));
        assert_eq!(after[0], (62.0, 30.0));
        assert_eq!(ends(&l.m)[0], (46.0, 30.0));
    }

    #[test]
    fn the_tint_fills_up_to_the_line_and_down_its_slope_just_past_the_baseline() {
        let mut s = quiet(5);
        s.push(Sample { o: 1.0, ..Default::default() });
        let l = live_line(&s, 0.0, 0, 400.0, G, true);
        let pts = ends(&l.fill);
        assert_eq!(&pts[..2], &[(46.0, 31.0), (46.0, 30.0)]);
        assert_eq!(*pts.last().expect("points"), (70.0, 31.0));
        assert_eq!(l.fill.last(), Some(&Cmd::Close));
        assert!(min(pts.iter().map(|p| p.1).collect()) < 30.0);
        assert_eq!(max(pts.iter().map(|p| p.1).collect()), 31.0);
    }

    #[test]
    fn the_unwritten_line_carries_on_from_the_end_back_down_to_the_baseline_to_the_edge() {
        assert_eq!(ends(&live_line(&quiet(6), 0.0, 0, 400.0, G, true).ahead), vec![(58.0, 30.0), (400.0, 30.0)]);
        let mut s = quiet(5);
        s.push(Sample { o: 1.0, ..Default::default() });
        let l = live_line(&s, 0.0, 0, 400.0, G, true);
        assert_eq!(l.ahead[0], Cmd::Move(l.pen));
        assert!(matches!(l.ahead[1], Cmd::Curve(..)));
        assert_eq!(ends(&l.ahead)[1..], [(70.0, 30.0), (400.0, 30.0)]);
        assert!(l.pen.y < 30.0);
    }
}
