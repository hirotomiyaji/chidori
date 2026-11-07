//! A list floating over everything, under its anchor: the source picker's lists, which show each
//! app's icon and its sound, as no native menu can. It is as wide as the anchor, or `min_width`;
//! `align_end` lines up its right edge with the anchor's. A press outside it, or Escape, closes it.

use gpui_kit::{
    Anchor, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement as _, Styled, anchored,
    deferred, div, px,
};

use crate::app::Message;
use crate::ui::{self, El, uid};

/// The gap between the anchor and the list, and the tallest it gets.
const GAP: f32 = 6.0;
const MAX_HEIGHT: f32 = 300.0;

pub struct Popover {
    anchor: El,
    popup: Option<El>,
    on_close: Message,
    min_width: f32,
    align_end: bool,
}

/// An anchor with its list open (`Some`) or closed.
pub fn popover(anchor: impl IntoElement, popup: Option<El>, on_close: Message) -> Popover {
    Popover { anchor: anchor.into_any_element(), popup, on_close, min_width: 0.0, align_end: false }
}

impl Popover {
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }

    pub fn align_end(mut self) -> Self {
        self.align_end = true;
        self
    }

    pub fn el(self) -> El {
        let mut root = div().relative().w_full().child(self.anchor);
        if let Some(popup) = self.popup {
            let close = self.on_close;
            let list = div()
                .id(uid("popover"))
                .min_w(px(self.min_width))
                .max_h(px(MAX_HEIGHT))
                .overflow_y_scroll()
                .occlude()
                .on_mouse_down_out(move |_, _, cx| ui::send(close.clone(), cx))
                .child(popup);
            let corner = if self.align_end { Anchor::TopRight } else { Anchor::TopLeft };
            let place = div().absolute().top_full().mt(px(GAP));
            let place = if self.align_end { place.right_0() } else { place.left_0().w_full() };
            root = root
                .child(place.child(deferred(anchored().anchor(corner).snap_to_window().child(list)).with_priority(1)));
        }
        root.into_any_element()
    }
}
