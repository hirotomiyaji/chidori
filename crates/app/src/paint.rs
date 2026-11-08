//! Drawing the session's line, the apps' sound and the mic test: paths, strokes, rectangles,
//! recorded into a frame and painted by a GPUI canvas. The vocabulary is the one the
//! drawing code was written in (Iced's canvas), so the geometry stays as it was.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{Bounds, Canvas, PathBuilder, PathStyle, Pixels, Window, canvas, point, px, size};
use lyon_tessellation::StrokeOptions;

use crate::theme::Color;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Seg {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}

/// Builds a path from its segments.
#[derive(Debug, Default)]
pub struct Builder {
    segs: Vec<Seg>,
}

impl Builder {
    pub fn move_to(&mut self, p: Point) {
        self.segs.push(Seg::Move(p));
    }

    pub fn line_to(&mut self, p: Point) {
        self.segs.push(Seg::Line(p));
    }

    pub fn bezier_curve_to(&mut self, c1: Point, c2: Point, to: Point) {
        self.segs.push(Seg::Cubic(c1, c2, to));
    }

    pub fn close(&mut self) {
        self.segs.push(Seg::Close);
    }

    /// A circle, as four cubic arcs.
    pub fn circle(&mut self, c: Point, r: f32) {
        let k = 0.552_284_8 * r;
        let (x, y) = (c.x, c.y);
        self.move_to(Point::new(x + r, y));
        self.bezier_curve_to(Point::new(x + r, y + k), Point::new(x + k, y + r), Point::new(x, y + r));
        self.bezier_curve_to(Point::new(x - k, y + r), Point::new(x - r, y + k), Point::new(x - r, y));
        self.bezier_curve_to(Point::new(x - r, y - k), Point::new(x - k, y - r), Point::new(x, y - r));
        self.bezier_curve_to(Point::new(x + k, y - r), Point::new(x + r, y - k), Point::new(x + r, y));
        self.close();
    }
}

#[derive(Debug, Clone, Default)]
pub struct Path {
    segs: Vec<Seg>,
}

impl Path {
    pub fn new(f: impl FnOnce(&mut Builder)) -> Self {
        let mut b = Builder::default();
        f(&mut b);
        Self { segs: b.segs }
    }

    pub fn circle(c: Point, r: f32) -> Self {
        Self::new(|p| p.circle(c, r))
    }

    fn is_empty(&self) -> bool {
        !self.segs.iter().any(|s| matches!(s, Seg::Line(_) | Seg::Cubic(..)))
    }

    fn build(&self, mut b: PathBuilder, at: gpui_kit::Point<Pixels>) -> Option<gpui_kit::Path<Pixels>> {
        let pt = |p: Point| point(at.x + px(p.x), at.y + px(p.y));
        for s in &self.segs {
            match *s {
                Seg::Move(p) => b.move_to(pt(p)),
                Seg::Line(p) => b.line_to(pt(p)),
                Seg::Cubic(c1, c2, to) => b.cubic_bezier_to(pt(to), pt(c1), pt(c2)),
                Seg::Close => b.close(),
            }
        }
        b.build().ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LineDash<'a> {
    pub segments: &'a [f32],
}

#[derive(Debug, Clone, Copy)]
pub struct Stroke<'a> {
    pub color: Color,
    pub width: f32,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    pub line_dash: LineDash<'a>,
}

impl Default for Stroke<'_> {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            line_dash: LineDash::default(),
        }
    }
}

impl Stroke<'_> {
    pub fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }

    pub fn with_width(self, width: f32) -> Self {
        Self { width, ..self }
    }

    pub fn with_line_cap(self, line_cap: LineCap) -> Self {
        Self { line_cap, ..self }
    }

    pub fn with_line_join(self, line_join: LineJoin) -> Self {
        Self { line_join, ..self }
    }
}

enum Op {
    Fill(Path, Color),
    Stroke { path: Path, color: Color, width: f32, cap: LineCap, join: LineJoin, dash: Vec<f32> },
    Rect(Point, Size, Color),
}

/// What a canvas draws, in order, in its own coordinates (0,0 at its top left).
pub struct Frame {
    ops: Vec<Op>,
    size: Size,
}

impl Frame {
    pub fn new(size: Size) -> Self {
        Self { ops: Vec::new(), size }
    }

    pub fn width(&self) -> f32 {
        self.size.width
    }

    pub fn height(&self) -> f32 {
        self.size.height
    }

    pub fn fill(&mut self, path: &Path, color: Color) {
        if color.a > 0.0 && !path.is_empty() {
            self.ops.push(Op::Fill(path.clone(), color));
        }
    }

    pub fn stroke(&mut self, path: &Path, s: Stroke<'_>) {
        if s.color.a > 0.0 && s.width > 0.0 && !path.is_empty() {
            self.ops.push(Op::Stroke {
                path: path.clone(),
                color: s.color,
                width: s.width,
                cap: s.line_cap,
                join: s.line_join,
                dash: s.line_dash.segments.to_vec(),
            });
        }
    }

    pub fn fill_rectangle(&mut self, at: Point, size: Size, color: Color) {
        if color.a > 0.0 && size.width > 0.0 && size.height > 0.0 {
            self.ops.push(Op::Rect(at, size, color));
        }
    }

    fn paint(self, at: gpui_kit::Point<Pixels>, window: &mut Window) {
        for op in self.ops {
            match op {
                Op::Fill(path, color) => {
                    if let Some(p) = path.build(PathBuilder::fill(), at) {
                        window.paint_path(p, color);
                    }
                }
                Op::Stroke { path, color, width, cap, join, dash } => {
                    let cap = match cap {
                        LineCap::Butt => lyon_tessellation::LineCap::Butt,
                        LineCap::Round => lyon_tessellation::LineCap::Round,
                    };
                    let join = match join {
                        LineJoin::Miter => lyon_tessellation::LineJoin::Miter,
                        LineJoin::Round => lyon_tessellation::LineJoin::Round,
                    };
                    let opts = StrokeOptions::default().with_line_width(width).with_line_cap(cap).with_line_join(join);
                    let mut b = PathBuilder::stroke(px(width)).with_style(PathStyle::Stroke(opts));
                    if !dash.is_empty() {
                        let d: Vec<Pixels> = dash.iter().map(|v| px(*v)).collect();
                        b = b.dash_array(&d);
                    }
                    if let Some(p) = path.build(b, at) {
                        window.paint_path(p, color);
                    }
                }
                Op::Rect(p, s, color) => {
                    let b = Bounds::new(point(at.x + px(p.x), at.y + px(p.y)), size(px(s.width), px(s.height)));
                    window.paint_quad(gpui_kit::fill(b, color));
                }
            }
        }
    }
}

/// A canvas that fills its box and draws `draw` into it each frame. `bounds`, when given, keeps
/// where it was laid out, for the pointer handlers around it.
pub fn canvas_el(
    bounds: Option<Rc<Cell<Bounds<Pixels>>>>,
    draw: impl FnOnce(&mut Frame) + 'static,
) -> Canvas<Bounds<Pixels>> {
    canvas(
        move |b, _, _| {
            if let Some(cell) = &bounds {
                cell.set(b);
            }
            b
        },
        move |_, b, window, _| {
            let mut frame = Frame::new(Size::new(b.size.width.into(), b.size.height.into()));
            draw(&mut frame);
            window.with_content_mask(Some(gpui_kit::ContentMask { bounds: b }), |window| {
                frame.paint(b.origin, window);
            });
        },
    )
}

/// A picture made in memory (a thumbnail, an app's icon), ready to draw.
#[derive(Clone)]
pub struct Picture(pub std::sync::Arc<gpui_kit::RenderImage>);

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Picture({:?})", self.0.id)
    }
}

impl Picture {
    /// From RGBA pixels; GPUI draws BGRA.
    pub fn from_rgba(width: u32, height: u32, mut rgba: Vec<u8>) -> Option<Self> {
        for px in rgba.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        let buf = image::RgbaImage::from_raw(width, height, rgba)?;
        let frame = image::Frame::new(buf);
        Some(Self(std::sync::Arc::new(gpui_kit::RenderImage::new(vec![frame]))))
    }
}
