//! The line across the window (DECISIONS → UI, "The line is real sound"). While recording it is
//! drawn from the real levels: 相手 rises above it, 自分 dips below it, screenshots are dots and a
//! pause leaves a dotted gap. A finished session's line is its whole timeline at once.

use chidori_core::session::model::{SourceId, TimelineItem};

use crate::paint::{Builder, Point};

use super::format::level_fraction;

/// One step of the live line (ten a second): each side's height, 0..1.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    pub o: f32,
    pub m: f32,
    pub shot: bool,
    pub gap: bool,
}

/// A level in dBFS as the line's height, 0..1; room noise stays flat.
pub fn height_of(dbfs: Option<f32>) -> f32 {
    let l = level_fraction(dbfs);
    ((l - 0.1) / 0.9).clamp(0.0, 1.0).powf(1.15)
}

/// One step toward a new height: quick to rise, slower to fall, like a meter.
pub fn settle(cur: f32, target: f32) -> f32 {
    cur + (target - cur) * if target > cur { 0.6 } else { 0.3 }
}

/// Ten minutes of steps; older ones have long scrolled out of view.
const MAX: usize = 6000;
/// A pause leaves this many empty steps in the line (0.8 s).
const GAP: usize = 8;

/// The live line's history. It belongs to the app, not a view, so a recording keeps its line
/// while 履歴 or 設定 is open.
#[derive(Debug, Default)]
pub struct LineHistory {
    pub session_id: Option<String>,
    pub samples: Vec<Sample>,
    /// When the newest step arrived, so the pen can glide between steps.
    pub at: Option<std::time::Instant>,
    o: f32,
    m: f32,
}

impl LineHistory {
    /// A new recording starts an empty line; the same one keeps its own.
    pub fn start(&mut self, session_id: &str) {
        if self.session_id.as_deref() == Some(session_id) {
            return;
        }
        *self = LineHistory {
            session_id: Some(session_id.to_string()),
            at: Some(std::time::Instant::now()),
            ..Default::default()
        };
    }

    /// One step from the levels (10 Hz): 相手 is the louder of app and system, 自分 the mic.
    pub fn push(&mut self, levels: &[(SourceId, f32)]) {
        let of = |id: SourceId| levels.iter().find(|(s, _)| *s == id).map(|(_, d)| *d);
        let others = height_of(of(SourceId::App)).max(height_of(of(SourceId::System)));
        self.o = settle(self.o, others);
        self.m = settle(self.m, height_of(of(SourceId::Mic)));
        self.samples.push(Sample { o: self.o, m: self.m, ..Default::default() });
        if self.samples.len() > MAX {
            self.samples.drain(..self.samples.len() - MAX);
        }
        self.at = Some(std::time::Instant::now());
    }

    /// A screenshot was taken: a dot at the newest step.
    pub fn shot(&mut self) {
        if let Some(s) = self.samples.last_mut() {
            s.shot = true;
        }
    }

    /// Recording resumed after a pause: a dotted gap in the line.
    pub fn gap(&mut self) {
        self.samples.extend(std::iter::repeat_n(Sample { gap: true, ..Default::default() }, GAP));
        self.o = 0.0;
        self.m = 0.0;
        self.at = Some(std::time::Instant::now());
    }
}

/// A whole session: each side's height per slice (0..1) and the slices holding a screenshot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shape {
    pub o: Vec<f32>,
    pub m: Vec<f32>,
    pub shots: Vec<usize>,
}

/// Slices in a session's shape; the same count as `SessionSummary.activity` from the core.
pub const SLICES: usize = 128;

/// A session's shape from its timeline: the share of each slice covered by each side's lines.
pub fn shape_of(items: &[TimelineItem], duration_ms: u64, n: usize) -> Shape {
    let mut end = duration_ms;
    for i in items {
        if let TimelineItem::Segment(s) = i {
            end = end.max(s.t_end_ms);
        }
    }
    let span = end.max(1) as f64 / n as f64;
    let mut o = vec![0f32; n];
    let mut m = vec![0f32; n];
    for i in items {
        let TimelineItem::Segment(s) = i else { continue };
        let side = if s.source == SourceId::Mic { &mut m } else { &mut o };
        let a = s.t_start_ms as f64;
        let b = (s.t_end_ms as f64).max(a);
        let k0 = ((a / span) as usize).min(n - 1);
        let k1 = ((b / span) as usize).min(n - 1);
        for (k, v) in side.iter_mut().enumerate().take(k1 + 1).skip(k0) {
            let cover = ((k + 1) as f64 * span).min(b) - (k as f64 * span).max(a);
            if cover > 0.0 {
                *v += (cover / span) as f32;
            }
        }
    }
    let mut shots: Vec<usize> = items
        .iter()
        .filter_map(|i| match i {
            TimelineItem::Screenshot(s) => Some(((s.t_ms as f64 / span) as usize).min(n - 1)),
            _ => None,
        })
        .collect();
    shots.sort_unstable();
    shots.dedup();
    Shape { o: o.into_iter().map(|v| v.min(1.0)).collect(), m: m.into_iter().map(|v| v.min(1.0)).collect(), shots }
}

/// A shape from `SessionSummary.activity` (percentages).
pub fn shape_from_activity(a: &chidori_core::session::recovery::Activity) -> Shape {
    Shape {
        o: a.others.iter().map(|v| *v as f32 / 100.0).collect(),
        m: a.me.iter().map(|v| *v as f32 / 100.0).collect(),
        shots: a.shots.iter().map(|s| *s as usize).collect(),
    }
}

/// A finished session's shape: how loud each side was (`sound`) when the levels were kept, else
/// when each side spoke; the screenshots always from its timeline, so deleting one shows.
pub fn session_shape(
    items: &[TimelineItem],
    duration_ms: u64,
    sound: Option<&chidori_core::session::recovery::Sound>,
) -> Shape {
    match sound {
        Some(sound) => {
            let n = sound.others.len().max(sound.me.len()).max(1);
            let shots = shape_of(items, duration_ms, n).shots;
            Shape {
                o: sound.others.iter().map(|v| *v as f32 / 100.0).collect(),
                m: sound.me.iter().map(|v| *v as f32 / 100.0).collect(),
                shots,
            }
        }
        None => shape_of(items, duration_ms, SLICES),
    }
}

/// A smooth path through the points (Catmull-Rom as cubic Béziers), continuing `b`. Starts with a
/// move to the first point unless `join`, which draws a line to it instead.
pub fn smooth(b: &mut Builder, pts: &[Point], join: bool) {
    let Some(first) = pts.first() else { return };
    if join {
        b.line_to(*first);
    } else {
        b.move_to(*first);
    }
    for i in 0..pts.len().saturating_sub(1) {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = *pts.get(i + 2).unwrap_or(&p2);
        b.bezier_curve_to(
            Point::new(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0),
            Point::new(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0),
            p2,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights() {
        assert_eq!(height_of(None), 0.0);
        assert_eq!(height_of(Some(-60.0)), 0.0);
        assert!((height_of(Some(0.0)) - 1.0).abs() < 1e-6);
        assert!(settle(0.0, 1.0) > settle(1.0, 0.0) - 0.5);
    }

    #[test]
    fn history_keeps_its_session() {
        let mut h = LineHistory::default();
        h.start("a");
        h.push(&[(SourceId::Mic, -10.0)]);
        h.start("a");
        assert_eq!(h.samples.len(), 1);
        h.gap();
        assert_eq!(h.samples.len(), 1 + GAP);
        h.start("b");
        assert!(h.samples.is_empty());
    }
}
