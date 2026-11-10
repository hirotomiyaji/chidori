//! The jello on screen (Jello brand, `jello.rs`): every element that moves between places, enters
//! or leaves goes through here. Each moving thing has a slot, kept between frames by its key; the
//! layout says where it should be, a canvas under it advances the spring once per frame and draws
//! the side (a darker shade of the element's colour, only while it moves), and the window asks for
//! another frame until everything rests. With reduced motion on (Windows' "Animation effects"
//! off), everything jumps straight to its place and no side is drawn.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Instant;

use gpui_kit::{
    Bounds, Div, IntoElement, ParentElement, PathBuilder, Pixels, SharedString, Styled, Window, canvas, div, fill,
    point, px, size,
};

use crate::jello::{Jello, Rect};
use crate::theme::Color;
use crate::ui::El;

struct Slot {
    jello: Jello,
    /// The frame it was last advanced in, for the time step.
    last: Option<Instant>,
    /// The render it was last built in; slots not built in the last render are dropped.
    seen: u64,
    /// For an element that enters: the target, from where it is laid out.
    origin: Option<(f64, f64)>,
}

thread_local! {
    static SLOTS: RefCell<HashMap<SharedString, Slot>> = RefCell::new(HashMap::new());
    static RENDER: Cell<u64> = const { Cell::new(0) };
}

/// A new render: slots its last render didn't build let go.
pub fn begin_frame() {
    let now = RENDER.with(|r| {
        r.set(r.get() + 1);
        r.get()
    });
    SLOTS.with(|s| s.borrow_mut().retain(|_, slot| slot.seen + 1 >= now));
}

fn touch(key: &SharedString) {
    let now = RENDER.with(Cell::get);
    SLOTS.with(|s| {
        if let Some(slot) = s.borrow_mut().get_mut(key) {
            slot.seen = now;
        }
    });
}

/// Keeps `key`'s slot for this render, for an element whose slot is aimed only while painting.
pub fn keep(key: &SharedString) {
    touch(key);
}

/// A width on the jello (the transcript scrollbar's thickness): the width this frame and whether
/// it still moves. Only the top is used; a side on a sliver a few pixels wide would be noise.
pub fn width(key: &SharedString, target: f32, instant: bool) -> (f32, bool) {
    aim(key, Rect::new(0.0, 0.0, f64::from(target), 0.0), None, instant);
    match advance(key, 0.0) {
        Some((top, _, moving)) => (top.w as f32, moving),
        None => (target, false),
    }
}

fn rect_of(b: Bounds<Pixels>, origin: gpui_kit::Point<Pixels>) -> Rect {
    Rect::new(
        f64::from(f32::from(b.origin.x - origin.x)),
        f64::from(f32::from(b.origin.y - origin.y)),
        f64::from(f32::from(b.size.width)),
        f64::from(f32::from(b.size.height)),
    )
}

/// Sends a slot to `target`, making it when new (`from` is where a new one starts; `None` puts it
/// there at once). Returns whether it still moves.
fn aim(key: &SharedString, target: Rect, from: Option<Rect>, instant: bool) -> bool {
    let now = RENDER.with(Cell::get);
    SLOTS.with(|s| {
        let mut s = s.borrow_mut();
        let slot = s.entry(key.clone()).or_insert_with(|| Slot {
            jello: Jello::new(from.unwrap_or(target)),
            last: None,
            seen: now,
            origin: None,
        });
        slot.seen = now;
        let t = slot.jello.target();
        let r = Rect::new(target.x.round(), target.y.round(), target.w.round(), target.h.round());
        if t != r {
            slot.jello.set(r, instant);
        }
        !slot.jello.resting()
    })
}

/// A slot's top this frame, its side outline (None while the layers line up), and whether it
/// still moves.
type Step = (Rect, Option<Vec<(f64, f64)>>, bool);

/// Advances a slot by the time since its last frame.
fn advance(key: &SharedString, radius: f32) -> Option<Step> {
    SLOTS.with(|s| {
        let mut s = s.borrow_mut();
        let slot = s.get_mut(key)?;
        let now = Instant::now();
        let dt = slot.last.map_or(1.0 / 60.0, |t| now.duration_since(t).as_secs_f64());
        let rest = slot.jello.frame(dt);
        slot.last = if rest { None } else { Some(now) };
        Some((slot.jello.top(), slot.jello.side_outline(f64::from(radius)), !rest))
    })
}

fn paint_side(points: &[(f64, f64)], origin: gpui_kit::Point<Pixels>, color: Color, window: &mut Window) {
    let at = |x: f64, y: f64| point(origin.x + px(x as f32), origin.y + px(y as f32));
    let mut path = PathBuilder::fill();
    for (i, (x, y)) in points.iter().enumerate() {
        if i == 0 {
            path.move_to(at(*x, *y));
        } else {
            path.line_to(at(*x, *y));
        }
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// What a sliding highlight looks like: its colour, its side's and its corner radius.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub color: Color,
    pub side: Color,
    pub radius: f32,
}

/// A highlight that slides between `items` (the active tab's pill, the chosen chip or source
/// row): `container` lays them out, and the one at `selected` gets the highlight under it, which
/// glides to the next one picked. The first placement lands at once.
pub fn slide(key: impl Into<SharedString>, container: Div, items: Vec<El>, selected: Option<usize>, look: Look) -> Div {
    let key: SharedString = key.into();
    touch(&key);
    let k2 = key.clone();
    let layer = canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let Some((top, side, moving)) = advance(&k2, look.radius) else { return };
            let reduce = cx.reduce_motion();
            if let Some(points) = side.filter(|_| !reduce) {
                paint_side(&points, bounds.origin, look.side, window);
            }
            let o = bounds.origin;
            let b = Bounds::new(
                point(o.x + px(top.x as f32), o.y + px(top.y as f32)),
                size(px(top.w as f32), px(top.h as f32)),
            );
            window.paint_quad(fill(b, look.color).corner_radii(px(look.radius)));
            if moving {
                window.request_animation_frame();
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let k3 = key.clone();
    container.relative().child(layer).children(items).on_children_prepainted(move |bounds, window, cx| {
        let (Some(origin), Some(sel)) = (bounds.first(), selected) else {
            SLOTS.with(|s| s.borrow_mut().remove(&k3));
            return;
        };
        let Some(b) = bounds.get(sel + 1) else { return };
        let target = rect_of(*b, origin.origin);
        if aim(&k3, target, None, cx.reduce_motion()) {
            window.request_animation_frame();
        }
    })
}

/// A switch's thumb, sliding along its track: drawn in `color` at `x` (its left edge, inside a
/// track `h` high), `size` square.
pub fn thumb(key: impl Into<SharedString>, x: f32, size_: f32, color: Color, side: Color) -> El {
    let key: SharedString = key.into();
    touch(&key);
    let radius = size_ / 2.0;
    let k2 = key.clone();
    let k3 = key.clone();
    let layer = canvas(
        move |bounds, window, cx| {
            let y = (f32::from(bounds.size.height) - size_) / 2.0;
            let target = Rect::new(f64::from(x), f64::from(y), f64::from(size_), f64::from(size_));
            if aim(&k3, target, None, cx.reduce_motion()) {
                window.request_animation_frame();
            }
        },
        move |bounds, _, window, cx| {
            let Some((top, side_pts, moving)) = advance(&k2, radius) else { return };
            if let Some(points) = side_pts.filter(|_| !cx.reduce_motion()) {
                paint_side(&points, bounds.origin, side, window);
            }
            let o = bounds.origin;
            let b = Bounds::new(
                point(o.x + px(top.x as f32), o.y + px(top.y as f32)),
                size(px(top.w as f32), px(top.h as f32)),
            );
            window.paint_quad(fill(b, color).corner_radii(px(radius)));
            if moving {
                window.request_animation_frame();
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    layer.into_any_element()
}

/// How an element enters: from `dx`, `dy` away from its place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct From {
    pub dx: f32,
    pub dy: f32,
}

/// An element that moves in from `from` (a toast from the bottom edge, the recording card from
/// where 開始 was, a new line from below) and, with `leaving`, out the same way. `from` is used
/// only when the slot is new: an element already on screen stays put. The jello runs in the
/// element's own frame, so the layout moving under it (a list scrolling) doesn't restart it.
pub fn enter(
    key: impl Into<SharedString>,
    content: impl IntoElement,
    from: Option<From>,
    leaving: bool,
    look: Look,
) -> El {
    let key: SharedString = key.into();
    touch(&key);
    let exists = SLOTS.with(|s| s.borrow().contains_key(&key));
    if !exists && from.is_none() {
        return div().child(content).into_any_element();
    }
    let away = from.unwrap_or(From { dx: 0.0, dy: 0.0 });
    // Where the top was last frame, relative to its place.
    let offset = SLOTS.with(|s| {
        s.borrow().get(&key).map(|slot| {
            let t = slot.jello.top();
            let o = slot.origin.unwrap_or((t.x, t.y));
            (t.x - o.0, t.y - o.1)
        })
    });
    let (ox, oy) = offset.unwrap_or((f64::from(away.dx), f64::from(away.dy)));
    let k2 = key.clone();
    let k3 = key.clone();
    let layer = canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let Some((_, side, moving)) = advance(&k2, look.radius) else { return };
            if let Some(points) = side.filter(|_| !cx.reduce_motion()) {
                paint_side(&points, bounds.origin, look.side, window);
            }
            if moving {
                window.request_animation_frame();
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let inner = div().relative().left(px(ox as f32)).top(px(oy as f32)).child(content);
    div()
        .relative()
        .child(layer)
        .child(inner)
        .on_children_prepainted(move |bounds, window, cx| {
            let Some(b) = bounds.first() else { return };
            let (w, h) = (f64::from(f32::from(b.size.width)), f64::from(f32::from(b.size.height)));
            let place = Rect::new(0.0, 0.0, w, h);
            let start = Rect::new(f64::from(away.dx), f64::from(away.dy), w, h);
            let target = if leaving { start } else { place };
            SLOTS.with(|s| {
                if let Some(slot) = s.borrow_mut().get_mut(&k3) {
                    slot.origin = Some((0.0, 0.0));
                }
            });
            if aim(&k3, target, Some(start), cx.reduce_motion()) {
                window.request_animation_frame();
            }
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_not_built_in_the_last_render_goes() {
        let key: SharedString = "test-slot".into();
        begin_frame();
        aim(&key, Rect::new(0.0, 0.0, 8.0, 8.0), None, false);
        begin_frame();
        assert!(SLOTS.with(|s| s.borrow().contains_key(&key)));
        begin_frame();
        assert!(!SLOTS.with(|s| s.borrow().contains_key(&key)));
    }

    #[test]
    fn a_new_slot_moves_from_where_it_enters() {
        let key: SharedString = "test-enter".into();
        begin_frame();
        let moving = aim(&key, Rect::new(0.0, 0.0, 80.0, 40.0), Some(Rect::new(0.0, 64.0, 80.0, 40.0)), false);
        assert!(moving);
        let (top, _, _) = advance(&key, 8.0).unwrap();
        assert!(top.y > 0.0 && top.y < 64.0);
        // Reduced motion: straight there.
        let key2: SharedString = "test-instant".into();
        aim(&key2, Rect::new(0.0, 0.0, 80.0, 40.0), Some(Rect::new(0.0, 64.0, 80.0, 40.0)), true);
        assert!(!aim(&key2, Rect::new(0.0, 0.0, 80.0, 40.0), None, true));
    }

    #[test]
    fn a_width_springs_toward_its_target() {
        let key: SharedString = "test-width".into();
        begin_frame();
        assert_eq!(width(&key, 4.0, false), (4.0, false));
        let (w, moving) = width(&key, 8.0, false);
        assert!(moving && w > 4.0 && w < 8.0);
        let key2: SharedString = "test-width-instant".into();
        width(&key2, 4.0, true);
        assert_eq!(width(&key2, 8.0, true), (8.0, false));
    }
}
