//! Pieces 設定, the model list and the setup wizard share: the big title with a line beside it,
//! the scrolling body, choices for a dropdown, small tags and cards, and fitting a summary into
//! its column.

use std::fmt;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Div, InteractiveElement, IntoElement, ParentElement, Styled, div, px};

use crate::app::Message;
use crate::theme::{self, Ink, line, p, tokens};
use crate::ui::{El, clickable, uid};

/// The big title on the ground, with a quiet line beside it (`設定` under a section's name, `2 / 6`
/// in the wizard). The hairline or rule under it belongs to the page.
pub fn head(title: String, sub: Option<String>) -> El {
    let mut r = div().flex().items_end().gap(px(10.0)).child(line(title, tokens::TITLE, p().ink));
    if let Some(sub) = sub {
        // Sits on the title's baseline rather than the bottom of its line box.
        r = r.child(line(sub, tokens::CAPTION, p().ink_muted).pb(px(4.0)));
    }
    div().w_full().flex_none().px(px(tokens::SPACE_4)).py(px(tokens::SPACE_3)).child(r).into_any_element()
}

/// A page body that scrolls, with the thin scrollbar of the rest of the app.
pub fn scroll(content: impl IntoElement) -> El {
    div().id(uid("scroll")).flex_1().min_h_0().w_full().overflow_y_scrollbar().child(content).into_any_element()
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

/// A small tag beside a name: outlined, or filled in ink for the one that stands out.
pub fn tag(label: String, filled: bool) -> El {
    let p = p();
    let fg = if filled { p.surface } else { p.ink_muted };
    let d = div()
        .flex_none()
        .px(px(6.0))
        .border_1()
        .border_color(if filled { p.ink } else { p.border })
        .rounded(px(tokens::RADIUS_SM))
        .child(line(label, tokens::CAPTION, fg).medium());
    if filled { d.bg(p.ink) } else { d }.into_any_element()
}

/// A card on the ground: raised, a hairline round it, or ink round the one in use.
pub fn card(content: impl IntoElement, chosen: bool) -> Div {
    card_style(div().w_full().p(px(tokens::SPACE_3)), chosen).child(content)
}

pub fn card_style<S: Styled>(d: S, chosen: bool) -> S {
    let p = p();
    let d = d.border_1().border_color(if chosen { p.ink } else { p.border }).rounded(px(tokens::RADIUS_LG));
    if chosen { d } else { d.bg(p.surface_raised) }
}

/// A radio mark: a ring, with the accent's dot when chosen.
pub fn radio_mark(checked: bool) -> El {
    let p = p();
    let dot = div().size(px(8.0)).rounded(px(4.0));
    let dot = if checked { dot.bg(p.accent) } else { dot };
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(16.0))
        .rounded(px(8.0))
        .bg(p.surface_raised)
        .border_1()
        .border_color(if checked { p.accent } else { p.border_strong })
        .child(dot)
        .into_any_element()
}

/// A quiet text button (ログを開く, ほかのモデル): muted, ink while hovered.
pub fn link(content: impl IntoElement, on: Message) -> El {
    let ink = p().ink;
    clickable(Some(on), "link")
        .px(px(4.0))
        .py(px(2.0))
        .rounded(px(tokens::RADIUS_SM))
        .text_color(p().ink_muted)
        .hover(move |s| s.text_color(ink))
        .child(content)
        .into_any_element()
}

/// A paragraph of body text.
pub fn para(s: String) -> El {
    theme::t(s, tokens::BODY, p().ink).into_any_element()
}

/// Shortens `s` with an ellipsis so it fits about `max_px` at `size` px: Japanese takes a full
/// em, Latin about half of one.
pub fn fit(s: &str, max_px: f32, size: f32) -> String {
    let width = |c: char| if c.is_ascii() { 0.55 } else { 1.0 } * size;
    let total: f32 = s.chars().map(width).sum();
    if total <= max_px {
        return s.to_string();
    }
    let room = max_px - width('…');
    let mut used = 0.0;
    let mut out = String::new();
    for c in s.chars() {
        used += width(c);
        if used > room {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn long_text_ends_in_an_ellipsis() {
        assert_eq!(fit("日本語", 210.0, 12.0), "日本語");
        let long = fit("Ctrl+Shift+R · Ctrl+Shift+S · Ctrl+Shift+M · Ctrl+Shift+X", 120.0, 12.0);
        assert!(long.ends_with('…'));
        assert!(long.chars().count() < 25);
    }
}
