//! The transcript's scrollbar: a thin grey thumb at the list's right edge that thickens on the jello
//! as the pointer comes near, and can be dragged or clicked to any point (FR-40). It reads GPUI's
//! own list numbers, so dragging away stops following the newest line and dragging to the end
//! follows it again. The kit's scrollbar thickens only on hover, off the jello, and leaves the
//! list's padding out of its range; see `docs/DECISIONS.md` → UI.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{
    Bounds, CursorStyle, DispatchPhase, Hitbox, HitboxBehavior, IntoElement, MouseButton, MouseDownEvent,
    MouseExitEvent, MouseMoveEvent, MouseUpEvent, SharedString, Styled, canvas, fill, point, px, size,
};

use crate::motion;
use crate::theme::{GRID, mix, p};
use crate::ui::El;

/// The thumb's thickness at rest.
const REST: f32 = 0.5 * GRID;
/// Its thickness with the pointer on it, or while dragged.
const FULL: f32 = GRID;
/// The gap between the thumb and the list's right edge; the thumb grows leftward from it.
const INSET: f32 = 0.5 * GRID;
/// The strip that takes the pointer: the rows' side padding, so it never covers a bubble.
const ZONE: f32 = 2.0 * GRID;
/// How near the pointer comes before the thumb starts to thicken.
const NEAR: f32 = 6.0 * GRID;
/// The shortest thumb, so a long session's stays easy to grab.
const MIN_LEN: f32 = 5.0 * GRID;
/// The track's margin at its top and bottom.
const MARGIN: f32 = GRID;

/// Where the thumb is drawn in a track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    track_top: f32,
    /// The thumb's length.
    len: f32,
    /// How far it can move.
    travel: f32,
    /// How far the list can scroll.
    extent: f32,
}

/// The thumb for a track `track_h` high from `track_top`, over a view `viewport_h` high with
/// `extent` more to scroll; `None` when there is nothing to scroll or no room for a thumb.
pub fn geometry(track_top: f32, track_h: f32, viewport_h: f32, extent: f32) -> Option<Geometry> {
    if extent <= 0.5 || track_h <= MIN_LEN || viewport_h <= 0.0 {
        return None;
    }
    let len = (track_h * viewport_h / (viewport_h + extent)).clamp(MIN_LEN, track_h);
    Some(Geometry { track_top, len, travel: track_h - len, extent })
}

impl Geometry {
    /// The thumb's top at scroll `offset`. While following, GPUI may report an offset past the end.
    pub fn thumb_top(&self, offset: f32) -> f32 {
        self.track_top + (offset / self.extent).clamp(0.0, 1.0) * self.travel
    }

    /// The scroll offset that puts the thumb's top at `y - grab`.
    pub fn offset_at(&self, y: f32, grab: f32) -> f32 {
        if self.travel <= 0.0 {
            return 0.0;
        }
        ((y - self.track_top - grab) / self.travel).clamp(0.0, 1.0) * self.extent
    }
}

/// The thumb's thickness with the pointer `dist` from the list's right edge: full in the strip,
/// at rest beyond `NEAR`, and growing in between.
pub fn thickness(dist: Option<f32>, dragging: bool) -> f32 {
    if dragging {
        return FULL;
    }
    let Some(d) = dist else { return REST };
    if d <= ZONE {
        FULL
    } else if d >= NEAR {
        REST
    } else {
        FULL - (FULL - REST) * (d - ZONE) / (NEAR - ZONE)
    }
}

/// A list's scrollbar state, shared with the handlers GPUI keeps between frames.
pub struct Bar {
    key: SharedString,
    /// Where in the thumb it was grabbed, while dragged.
    drag: Cell<Option<f32>>,
    /// The thickness last aimed at, so a moving pointer redraws only when it changes.
    aimed: Cell<f32>,
}

impl Bar {
    pub fn new(name: &str) -> Self {
        Self { key: format!("{name}-bar").into(), drag: Cell::new(None), aimed: Cell::new(REST) }
    }

    pub fn dragging(&self) -> bool {
        self.drag.get().is_some()
    }

    /// Lets go of a drag (a different session).
    pub fn reset(&self) {
        self.drag.set(None);
    }
}

/// How far the pointer at (`x`, `y`) is from the right edge of `b`, when it is beside the list.
fn distance(b: Bounds<gpui_kit::Pixels>, x: f32, y: f32) -> Option<f32> {
    let (top, bottom, right) = (f32::from(b.top()), f32::from(b.bottom()), f32::from(b.right()));
    (y >= top && y <= bottom && x <= right && x >= f32::from(b.left())).then_some(right - x)
}

/// The scrollbar over a list, `top` being the room the floating card takes over the list's top.
/// It goes after the list, so it is on top of the rows and takes the press first.
pub fn bar(items: &gpui_kit::ListState, bar: &Rc<Bar>, top: f32) -> El {
    motion::keep(&bar.key);
    let (items_p, bar_p) = (items.clone(), bar.clone());
    canvas(
        move |bounds, window, _| {
            let view = f32::from(items_p.viewport_bounds().size.height);
            let extent = f32::from(items_p.max_offset_for_scrollbar().y);
            let offset = -f32::from(items_p.scroll_px_offset_for_scrollbar().y);
            let track_top = f32::from(bounds.top()) + top + MARGIN;
            let track_h = f32::from(bounds.bottom()) - MARGIN - track_top;
            let g = geometry(track_top, track_h, view, extent)?;
            let strip = Bounds::new(point(bounds.right() - px(ZONE), px(track_top)), size(px(ZONE), px(track_h)));
            let hitbox = window.insert_hitbox(strip, HitboxBehavior::BlockMouseExceptScroll);
            let m = window.mouse_position();
            let target = thickness(distance(bounds, f32::from(m.x), f32::from(m.y)), bar_p.dragging());
            bar_p.aimed.set(target);
            Some((g, offset, hitbox, target))
        },
        {
            let (items, bar) = (items.clone(), bar.clone());
            move |bounds, state: Option<(Geometry, f32, Hitbox, f32)>, window, cx| {
                let Some((g, offset, hitbox, target)) = state else { return };
                let (w, moving) = motion::width(&bar.key, target, cx.reduce_motion());
                if moving {
                    window.request_animation_frame();
                }
                let right = f32::from(bounds.right()) - INSET;
                let thumb = Bounds::new(point(px(right - w), px(g.thumb_top(offset))), size(px(w), px(g.len)));
                let heat = ((w - REST) / (FULL - REST)).clamp(0.0, 1.0);
                let color = if bar.dragging() { p().muted } else { mix(p().muted, p().faint, heat) };
                window.paint_quad(fill(thumb, color).corner_radii(px(w / 2.0)));
                window.set_cursor_style(CursorStyle::Arrow, &hitbox);

                let (it, b) = (items.clone(), bar.clone());
                window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || e.button != MouseButton::Left || !hitbox.is_hovered(window) {
                        return;
                    }
                    cx.stop_propagation();
                    let y = f32::from(e.position.y);
                    let at = g.thumb_top(offset);
                    let grab = if y >= at && y <= at + g.len {
                        y - at
                    } else {
                        // A press on the track jumps there, and goes on as a drag.
                        let grab = g.len / 2.0;
                        it.set_offset_from_scrollbar(point(px(0.0), px(-g.offset_at(y, grab))));
                        grab
                    };
                    it.scrollbar_drag_started();
                    b.drag.set(Some(grab));
                    window.refresh();
                });
                let (it, b) = (items.clone(), bar.clone());
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    if let Some(grab) = b.drag.get() {
                        if e.pressed_button != Some(MouseButton::Left) {
                            // Let go outside the window.
                            it.scrollbar_drag_ended();
                            b.drag.set(None);
                        } else {
                            it.set_offset_from_scrollbar(point(
                                px(0.0),
                                px(-g.offset_at(f32::from(e.position.y), grab)),
                            ));
                            cx.stop_propagation();
                        }
                        window.refresh();
                        return;
                    }
                    let d = distance(bounds, f32::from(e.position.x), f32::from(e.position.y));
                    if thickness(d, false) != b.aimed.get() {
                        window.refresh();
                    }
                });
                let (it, b) = (items.clone(), bar.clone());
                window.on_mouse_event(move |e: &MouseUpEvent, phase, window, _| {
                    if phase == DispatchPhase::Bubble && e.button == MouseButton::Left && b.dragging() {
                        it.scrollbar_drag_ended();
                        b.drag.set(None);
                        window.refresh();
                    }
                });
                let b = bar.clone();
                window.on_mouse_event(move |_: &MouseExitEvent, _, window, _| {
                    if !b.dragging() && b.aimed.get() != REST {
                        window.refresh();
                    }
                });
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reset_lets_go_of_a_drag() {
        let bar = Bar::new("t");
        bar.drag.set(Some(4.0));
        assert!(bar.dragging());
        bar.reset();
        assert!(!bar.dragging());
    }

    #[test]
    fn nothing_to_scroll_has_no_thumb() {
        assert!(geometry(0.0, 400.0, 400.0, 0.0).is_none());
        assert!(geometry(0.0, MIN_LEN, 400.0, 900.0).is_none());
    }

    #[test]
    fn the_thumb_is_as_long_as_the_share_in_view() {
        let g = geometry(0.0, 400.0, 400.0, 400.0).unwrap();
        assert_eq!(g.len, 200.0);
        // A very long session keeps a thumb that can be grabbed.
        assert_eq!(geometry(0.0, 400.0, 400.0, 1e6).unwrap().len, MIN_LEN);
    }

    #[test]
    fn the_thumb_runs_from_the_top_to_the_end_of_the_track() {
        let g = geometry(10.0, 400.0, 400.0, 400.0).unwrap();
        assert_eq!(g.thumb_top(0.0), 10.0);
        assert_eq!(g.thumb_top(400.0), 210.0);
        // Following reports an offset past the end; the thumb stays at the bottom.
        assert_eq!(g.thumb_top(450.0), 210.0);
    }

    #[test]
    fn dragging_keeps_the_thumb_under_the_pointer() {
        let g = geometry(10.0, 400.0, 300.0, 900.0).unwrap();
        for o in [0.0, 120.0, 450.0, 900.0] {
            let grab = 12.0;
            assert!((g.offset_at(g.thumb_top(o) + grab, grab) - o).abs() < 0.01);
        }
    }

    #[test]
    fn a_press_on_the_track_centres_the_thumb_there() {
        let g = geometry(0.0, 400.0, 400.0, 400.0).unwrap();
        let o = g.offset_at(200.0, g.len / 2.0);
        assert_eq!(g.thumb_top(o) + g.len / 2.0, 200.0);
    }

    #[test]
    fn the_thumb_thickens_as_the_pointer_comes_near() {
        assert_eq!(thickness(None, false), REST);
        assert_eq!(thickness(Some(NEAR + 10.0), false), REST);
        assert_eq!(thickness(Some(ZONE), false), FULL);
        assert_eq!(thickness(Some(0.0), false), FULL);
        assert_eq!(thickness(None, true), FULL);
        let mut last = REST;
        for d in (0..=NEAR as i32).rev() {
            let t = thickness(Some(d as f32), false);
            assert!(t >= last);
            last = t;
        }
    }
}
