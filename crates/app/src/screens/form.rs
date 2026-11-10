//! Pieces 設定, the model list and the setup wizard share: the title with a line beside it, the
//! scrolling body, choices for a dropdown, small tags and cards.

use std::fmt;

use gpui_kit::{Div, InteractiveElement, IntoElement, ParentElement, Styled, div, px};

use crate::app::Message;
use crate::theme::{self, Ink, R_PANEL, R_TAG, g, line, p, tokens};
use crate::ui::{El, clickable};

/// The title on the ground, with a quiet line beside it (`2 / 6` in the wizard).
pub fn head(title: String, sub: Option<String>) -> El {
    let mut r = div().flex().items_end().gap(g(1.0)).child(line(title, tokens::TITLE, p().ink));
    if let Some(sub) = sub {
        // Sits on the title's baseline rather than the bottom of its line box.
        r = r.child(line(sub, tokens::SMALL, p().muted).pb(px(4.0)));
    }
    div().w_full().flex_none().px(g(3.0)).py(g(1.0)).child(r).into_any_element()
}

/// A page body that scrolls, with the app's scrollbar. `key` names it: it keeps its place while
/// shown, and another key (another tab or step) starts at the top.
pub fn scroll(key: &str, content: impl IntoElement) -> El {
    crate::scrollbar::area(key, content)
}

/// One choice in a dropdown: what is saved, and what it says.
#[derive(Debug, Clone)]
pub struct Opt<T> {
    pub value: T,
    pub label: String,
}

impl<T: PartialEq> PartialEq for Opt<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T> fmt::Display for Opt<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

pub fn opt<T>(value: T, label: impl Into<String>) -> Opt<T> {
    Opt { value, label: label.into() }
}

/// A dropdown over `options` showing `value`; a pick sends what `on` makes of its value. `key`
/// names the dropdown.
pub fn choose<T>(key: &str, options: Vec<Opt<T>>, value: &T, on: impl Fn(T) -> Message) -> El
where
    T: Clone + PartialEq + 'static,
{
    let selected = options.iter().find(|o| o.value == *value).cloned();
    crate::widgets::select(key, options, selected, move |o: Opt<T>| on(o.value))
}

/// A small tag beside a name: on the ground, or in ink for the one that stands out.
pub fn tag(label: String, filled: bool) -> El {
    let p = p();
    let (bg, fg) = if filled { (p.ink, p.on_ink) } else { (p.ground, p.muted) };
    div()
        .flex_none()
        .px(g(1.0))
        .rounded(px(R_TAG))
        .bg(bg)
        .child(line(label, tokens::SMALL, fg).bold())
        .into_any_element()
}

/// A card: a white panel, with the ink ring round the one in use.
pub fn card(content: impl IntoElement, chosen: bool) -> Div {
    card_style(div().w_full().p(g(2.0)), chosen).child(content)
}

pub fn card_style<S: Styled>(d: S, chosen: bool) -> S {
    let p = p();
    let d = d.bg(p.surface).rounded(px(R_PANEL));
    if chosen { d.border_2().border_color(p.ink) } else { d }
}

/// A radio mark: a ring, with an ink dot when chosen.
pub fn radio_mark(checked: bool) -> El {
    let p = p();
    let dot = div().size(g(1.0)).rounded(px(4.0));
    let dot = if checked { dot.bg(p.ink) } else { dot };
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(g(2.0))
        .rounded(g(1.0))
        .border_2()
        .border_color(if checked { p.ink } else { p.line })
        .child(dot)
        .into_any_element()
}

/// A quiet text button (ログを開く, ほかのモデル): muted, ink while hovered.
pub fn link(content: impl IntoElement, on: Message) -> El {
    let ink = p().ink;
    clickable(Some(on), "link")
        .flex()
        .items_center()
        .h(g(4.0))
        .px(g(1.0))
        .rounded(g(2.0))
        .text_color(p().muted)
        .hover(move |s| s.text_color(ink))
        .child(content)
        .into_any_element()
}

/// A paragraph of body text.
pub fn para(s: String) -> El {
    theme::t(s, tokens::BODY, p().ink).into_any_element()
}
