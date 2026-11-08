//! The Jello motion, ported from the brand's `motion/jello.js` with its numbers and step order
//! unchanged; the tests below replay the brand's recorded moves (`brand/jello-golden/`) against it.
//! `motion.rs` puts it on screen.
//!
//! Two layers: the top (the UI) goes straight to its target on a critically damped spring; the
//! base chases the top on a softer one, wobbling and stretching. Only the side between them is
//! drawn, and only while they are apart.

const TOP_STIFFNESS: f64 = 380.0;
const TOP_DAMPING_RATIO: f64 = 1.0;
const BASE_STIFFNESS: f64 = 520.0;
const WOBBLE: f64 = 0.75;
const STRETCH_RATE: f64 = 0.03;
const SPEED: f64 = 1.0;
const SUBSTEPS: u32 = 4;
const MAX_FRAME: f64 = 1.0 / 30.0;
const SETTLE_DISTANCE: f64 = 0.05;
const SETTLE_VELOCITY: f64 = 0.8;
const SIDE_VISIBLE: f64 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }

    fn get(&self, k: usize) -> f64 {
        [self.x, self.y, self.w, self.h][k]
    }

    fn get_mut(&mut self, k: usize) -> &mut f64 {
        match k {
            0 => &mut self.x,
            1 => &mut self.y,
            2 => &mut self.w,
            _ => &mut self.h,
        }
    }

    fn rounded(self) -> Self {
        Rect::new(self.x.round(), self.y.round(), self.w.round(), self.h.round())
    }
}

#[derive(Debug, Clone, Copy)]
struct Layer {
    p: Rect,
    v: Rect,
}

impl Layer {
    fn at(r: Rect) -> Self {
        Layer { p: r, v: Rect::default() }
    }
}

/// One element moving the jello way.
#[derive(Debug, Clone)]
pub struct Jello {
    target: Rect,
    top: Layer,
    base: Layer,
    resting: bool,
}

impl Jello {
    pub fn new(rect: Rect) -> Self {
        let target = rect.rounded();
        Jello { target, top: Layer::at(target), base: Layer::at(target), resting: true }
    }

    /// Sends the element to a new rect; `instant` (reduced motion) jumps there.
    pub fn set(&mut self, next: Rect, instant: bool) {
        self.target = next.rounded();
        if instant {
            self.top = Layer::at(self.target);
            self.base = Layer::at(self.target);
            self.resting = true;
        } else {
            self.resting = false;
        }
    }

    /// Advances one frame of `seconds`. Returns true once at rest.
    pub fn frame(&mut self, seconds: f64) -> bool {
        if self.resting {
            return true;
        }
        let dt = seconds.min(MAX_FRAME) * SPEED / f64::from(SUBSTEPS);
        for _ in 0..SUBSTEPS {
            self.substep(dt);
        }
        if self.settled() {
            self.top = Layer::at(self.target);
            self.base = Layer::at(self.target);
            self.resting = true;
        }
        self.resting
    }

    fn substep(&mut self, dt: f64) {
        let c0 = 2.0 * TOP_STIFFNESS.sqrt() * TOP_DAMPING_RATIO;
        let c1 = 2.0 * BASE_STIFFNESS.sqrt() * base_damping_ratio();
        for k in 0..4 {
            let (p, v) = (self.top.p.get(k), self.top.v.get(k));
            let v = v + (-TOP_STIFFNESS * (p - self.target.get(k)) - c0 * v) * dt;
            *self.top.v.get_mut(k) = v;
            *self.top.p.get_mut(k) = p + v * dt;
        }
        for k in 0..4 {
            let (p, v) = (self.base.p.get(k), self.base.v.get(k));
            let v = v + (-BASE_STIFFNESS * (p - self.top.p.get(k)) - c1 * v) * dt;
            *self.base.v.get_mut(k) = v;
            *self.base.p.get_mut(k) = p + v * dt;
        }
    }

    fn settled(&self) -> bool {
        (0..4).all(|k| {
            (self.top.p.get(k) - self.target.get(k)).abs() <= SETTLE_DISTANCE
                && self.top.v.get(k).abs() <= SETTLE_VELOCITY
                && (self.base.p.get(k) - self.top.p.get(k)).abs() <= SETTLE_DISTANCE
                && self.base.v.get(k).abs() <= SETTLE_VELOCITY
        })
    }

    pub fn resting(&self) -> bool {
        self.resting
    }

    pub fn target(&self) -> Rect {
        self.target
    }

    /// Where to draw the UI.
    pub fn top(&self) -> Rect {
        self.top.p
    }

    /// The base, stretched along its velocity; only used to draw the side.
    pub fn base(&self) -> Rect {
        stretch(self.base.p, self.base.v)
    }

    /// The side as one filled outline with its rounded corners already in it, for renderers
    /// whose paths have no round stroke joins.
    pub fn side_outline(&self, radius: f64) -> Option<Vec<(f64, f64)>> {
        side_outline(self.top(), self.base(), radius)
    }
}

fn base_damping_ratio() -> f64 {
    (1.0 - 0.85 * WOBBLE).max(0.08)
}

/// Squash and stretch along the velocity, capped so a fast move never turns into a streak.
pub fn stretch(r: Rect, v: Rect) -> Rect {
    let ex = (v.x.abs() * STRETCH_RATE).min(8.0 + 0.3 * r.w);
    let ey = (v.y.abs() * STRETCH_RATE).min(8.0 + 0.3 * r.h);
    let gx = ex - (ey * 0.5).min(0.08 * r.w + 4.0);
    let gy = ey - (ex * 0.5).min(0.08 * r.h + 4.0);
    Rect::new(r.x - gx / 2.0, r.y - gy / 2.0, (r.w + gx).max(2.0), (r.h + gy).max(2.0))
}

/// The convex hull of both rects inset by `radius`. Draw it filled and stroked at 2 × radius
/// with round joins in a darker shade of the top's colour.
pub fn side(top: Rect, base: Rect, radius: f64) -> Option<Vec<(f64, f64)>> {
    if (0..4).all(|k| (top.get(k) - base.get(k)).abs() <= SIDE_VISIBLE) {
        return None;
    }
    let rad = radius.min(top.w / 2.0).min(top.h / 2.0).min(base.w / 2.0).min(base.h / 2.0).max(0.0);
    let inset = |r: Rect| {
        [
            (r.x + rad, r.y + rad),
            (r.x + r.w - rad, r.y + rad),
            (r.x + r.w - rad, r.y + r.h - rad),
            (r.x + rad, r.y + r.h - rad),
        ]
    };
    let mut points: Vec<(f64, f64)> = inset(top).into_iter().chain(inset(base)).collect();
    Some(hull(&mut points))
}

/// [`side`] with the stroke baked in: the hull of both rects' rounded corners, sampled at 8
/// points per quarter circle.
pub fn side_outline(top: Rect, base: Rect, radius: f64) -> Option<Vec<(f64, f64)>> {
    side(top, base, radius)?;
    let rad = radius.min(top.w / 2.0).min(top.h / 2.0).min(base.w / 2.0).min(base.h / 2.0).max(0.0);
    let mut points = Vec::with_capacity(72);
    for r in [top, base] {
        let corners = [
            (r.x + rad, r.y + rad, std::f64::consts::PI),
            (r.x + r.w - rad, r.y + rad, 1.5 * std::f64::consts::PI),
            (r.x + r.w - rad, r.y + r.h - rad, 0.0),
            (r.x + rad, r.y + r.h - rad, 0.5 * std::f64::consts::PI),
        ];
        for (cx, cy, start) in corners {
            for i in 0..=8 {
                let a = start + f64::from(i) / 8.0 * std::f64::consts::FRAC_PI_2;
                points.push((cx + rad * a.cos(), cy + rad * a.sin()));
            }
        }
    }
    Some(hull(&mut points))
}

fn hull(pts: &mut [(f64, f64)]) -> Vec<(f64, f64)> {
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut lower: Vec<(f64, f64)> = Vec::new();
    for &p in pts.iter() {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(f64, f64)> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    upper.pop();
    lower.pop();
    lower.extend(upper);
    lower
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_shows_only_while_apart() {
        let mut j = Jello::new(Rect::new(0.0, 0.0, 100.0, 40.0));
        assert!(j.side_outline(8.0).is_none());
        j.set(Rect::new(0.0, 80.0, 100.0, 40.0), false);
        j.frame(1.0 / 60.0);
        j.frame(1.0 / 60.0);
        let outline = j.side_outline(8.0).expect("apart while moving");
        // The outline covers both rects: from the top of the base to the bottom of the top.
        let (min_y, max_y) = outline.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        assert!(min_y <= j.base().y + 0.01 && max_y >= j.top().y + j.top().h - 0.01);
        while !j.frame(1.0 / 60.0) {}
        assert!(j.side_outline(8.0).is_none());
        assert_eq!(j.top(), Rect::new(0.0, 80.0, 100.0, 40.0));
    }

    fn rect(v: &serde_json::Value) -> Rect {
        let f = |k: &str| v[k].as_f64().unwrap();
        Rect::new(f("x"), f("y"), f("w"), f("h"))
    }

    fn assert_close(what: &str, frame: usize, got: Rect, want: Rect) {
        for (g, w) in [(got.x, want.x), (got.y, want.y), (got.w, want.w), (got.h, want.h)] {
            assert!((g - w).abs() <= 0.5, "{what} frame {frame}: got {got:?}, want {want:?}");
        }
    }

    /// Every sample of the top and the stretched base within 0.5 px of the brand's recording.
    #[test]
    fn matches_the_golden_moves() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("brand/jello-golden");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let golden: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut jello = Jello::new(rect(&golden["from"]));
            jello.set(rect(&golden["to"]), false);
            let frames = golden["frames"].as_array().unwrap();
            for (i, f) in frames.iter().enumerate() {
                jello.frame(1.0 / 60.0);
                assert_close(&format!("{name} top"), i, jello.top(), rect(&f["top"]));
                assert_close(&format!("{name} base"), i, jello.base(), rect(&f["base"]));
            }
            assert!(jello.resting(), "{name} should rest after its last frame");
            checked += 1;
        }
        assert!(checked >= 4);
    }
}
