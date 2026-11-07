//! The kit's building blocks in GPUI: buttons (the primary with the kamon's inner line), switches,
//! fields, the dialog sheet in the frame, the rule. Every value comes from the tokens; colours are
//! read from the palette when the view is built.

use std::cell::RefCell;
use std::time::Instant;

use gpui_kit::{
    BoxShadow, Div, InteractiveElement, IntoElement, ParentElement, Stateful, StatefulInteractiveElement, Styled,
    anchored, deferred, div, point, px,
};

use crate::app::Message;
use crate::icons::{Icon, icon, turned};
use crate::theme::{self, Color, Ink, alpha, line, p, plain, t, tokens};
use crate::ui::{self, El, clickable};

pub use crate::ui::with_tip;

/// A row: children side by side, centred across.
pub fn row() -> Div {
    div().flex().flex_row().items_center()
}

/// A column: children one under another.
pub fn col() -> Div {
    div().flex().flex_col()
}

/// Space that takes what is left in a row or column.
pub fn fill() -> Div {
    div().flex_1()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// The one accent-filled action of a view, with the kamon's inner line.
    Primary,
    /// A destructive confirmation: the primary's shape in the brand's brick.
    Danger,
    Secondary,
    Ghost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Sm,
    Md,
    Lg,
}

impl Size {
    fn height(self) -> f32 {
        match self {
            Size::Sm => 28.0,
            Size::Md => 32.0,
            Size::Lg => 40.0,
        }
    }

    fn pad_x(self) -> f32 {
        match self {
            Size::Sm => 10.0,
            Size::Md => tokens::SPACE_3,
            Size::Lg => tokens::SPACE_4,
        }
    }
}

/// A variant's ground, hovered ground, text and border.
fn colors(variant: Variant) -> (Color, Color, Color, Option<Color>) {
    let p = p();
    match variant {
        Variant::Primary => (p.accent, p.accent_hover, p.on_accent, None),
        Variant::Danger => (p.danger, p.danger, p.surface, None),
        Variant::Secondary => (p.surface_raised, p.surface_sunken, p.ink, Some(p.border_strong)),
        Variant::Ghost => (Color::TRANSPARENT, p.surface_sunken, p.ink, None),
    }
}

/// The text colour on a variant.
pub fn on_color(variant: Variant) -> Color {
    colors(variant).2
}

/// A button's box in a variant's look around any content. Primary and danger get the kamon's inner
/// line: 1 px in the text colour, 3 px inside the edge.
fn button_box(
    content: impl IntoElement,
    variant: Variant,
    height: f32,
    pad_x: f32,
    on: Option<Message>,
) -> Stateful<Div> {
    let (bg, hover, fg, border) = colors(variant);
    let enabled = on.is_some();
    let seed = ("btn", variant as u8);
    let mut b =
        clickable(on, seed).flex().flex_none().h(px(height)).rounded(px(tokens::RADIUS_MD)).bg(bg).text_color(fg);
    if let Some(c) = border {
        b = b.border_1().border_color(c);
    }
    if enabled {
        b = b.hover(move |s| s.bg(hover));
    } else {
        b = b.opacity(0.45);
    }
    let lined = matches!(variant, Variant::Primary | Variant::Danger);
    if lined {
        b.p(px(2.0)).child(
            row()
                .flex_1()
                .justify_center()
                .gap(px(6.0))
                .px(px(pad_x - 3.0))
                .border_1()
                .border_color(fg)
                .rounded(px(tokens::RADIUS_MD - 3.0))
                .child(content),
        )
    } else {
        b.child(row().flex_1().justify_center().gap(px(6.0)).px(px(pad_x)).child(content))
    }
}

/// A labelled button. `on` is `None` for a disabled one.
pub fn btn(label: impl Into<String>, variant: Variant, size: Size, on: Option<Message>) -> Stateful<Div> {
    btn_with(plain(label.into(), tokens::LABEL), variant, size, on)
}

/// A button with an icon before its label.
pub fn btn_icon(
    which: Icon,
    label: impl Into<String>,
    variant: Variant,
    size: Size,
    on: Option<Message>,
) -> Stateful<Div> {
    let fg = on_color(variant);
    btn_with(
        row().gap(px(6.0)).child(icon(which, 14.0, fg)).child(plain(label.into(), tokens::LABEL)),
        variant,
        size,
        on,
    )
}

/// A button around any content.
pub fn btn_with(content: impl IntoElement, variant: Variant, size: Size, on: Option<Message>) -> Stateful<Div> {
    button_box(content, variant, size.height(), size.pad_x(), on)
}

/// An icon-only button with its name as a tooltip (the React UI's `IconButton`).
pub fn icon_button(which: Icon, label: impl Into<String>, size: Size, on: Option<Message>) -> El {
    icon_button_tinted(which, label, size, on, None)
}

/// An icon button whose icon has a colour of its own while `tint` is set (a pressed pin, ...).
pub fn icon_button_tinted(
    which: Icon,
    label: impl Into<String>,
    size: Size,
    on: Option<Message>,
    tint: Option<Color>,
) -> El {
    let side = if size == Size::Sm { 28.0 } else { 32.0 };
    let glyph = if size == Size::Sm { 14.0 } else { 16.0 };
    let enabled = on.is_some();
    let group = ui::uid(("icon-btn-group", which as u8));
    let group = format!("{group:?}");
    let mut svg = icon(which, glyph, tint.unwrap_or(p().ink_muted));
    if tint.is_none() {
        let ink = p().ink;
        svg = svg.group_hover(group.clone(), move |s| s.text_color(ink));
    }
    let mut b = clickable(on, ("icon-btn", which as u8))
        .group(group)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded(px(tokens::RADIUS_MD))
        .child(svg);
    if enabled {
        let sunk = p().surface_sunken;
        b = b.hover(move |s| s.bg(sunk));
    } else {
        b = b.opacity(0.4);
    }
    with_tip(b, label.into()).into_any_element()
}

/// A sheet that floats over content: raised, hairline, the brand's one shadow.
pub fn floating(d: Div) -> Div {
    let p = p();
    d.bg(p.surface_raised)
        .text_color(p.ink)
        .border_1()
        .border_color(p.border)
        .rounded(px(tokens::RADIUS_LG))
        .shadow(vec![shadow()])
}

pub fn shadow() -> BoxShadow {
    BoxShadow {
        color: if theme::is_dark() { alpha(Color::BLACK, 0.4) } else { alpha(p().ink, 0.08) }.into(),
        offset: point(px(0.0), px(4.0)),
        blur_radius: px(16.0),
        spread_radius: px(0.0),
        inset: false,
    }
}

fn switch_track(on: bool, enabled: bool) -> Div {
    // A switch that can't be changed fades as a whole, track and knob.
    let a = if enabled { 1.0 } else { 0.5 };
    let knob = div().size(px(16.0)).rounded(px(4.0)).bg(alpha(p().surface_raised, a));
    let track = div()
        .flex()
        .flex_none()
        .w(px(36.0))
        .h(px(20.0))
        .p(px(2.0))
        .rounded(px(tokens::RADIUS_SM))
        .bg(alpha(if on { p().accent } else { p().border_strong }, a));
    if on { track.justify_end().child(knob) } else { track.justify_start().child(knob) }
}

/// The switch: a squared track, never a pill (`radius-full` is for people).
pub fn switch(on: bool, msg: Option<Message>) -> El {
    let enabled = msg.is_some();
    clickable(msg, ("switch", on)).child(switch_track(on, enabled)).into_any_element()
}

/// A row that toggles: its label, then the switch.
pub fn toggle_row(label: impl Into<String>, on: bool, msg: Option<Message>) -> El {
    let enabled = msg.is_some();
    let color = if enabled { p().ink } else { alpha(p().ink, 0.5) };
    let label: String = label.into();
    clickable(msg, ("toggle", &label))
        .flex()
        .items_center()
        .gap(px(tokens::SPACE_3))
        .py(px(tokens::SPACE_2))
        .child(t(label, tokens::BODY_SMALL, color).flex_1())
        .child(switch_track(on, enabled))
        .into_any_element()
}

/// A labelled setting.
pub fn field(label: impl Into<String>, content: impl IntoElement) -> El {
    col()
        .gap(px(6.0))
        .py(px(tokens::SPACE_2))
        .child(line(label.into(), tokens::CAPTION, p().ink_muted).medium())
        .child(content)
        .into_any_element()
}

thread_local! {
    static OPEN_SELECT: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// A dropdown of choices (the kit's select): the chosen one in a box that opens a list under it.
/// `key` names it, so only one is open at a time.
pub fn select<T>(key: &str, options: Vec<T>, selected: Option<T>, on_select: impl Fn(T) -> Message) -> El
where
    T: ToString + PartialEq + Clone + 'static,
{
    let key = key.to_string();
    let open = OPEN_SELECT.with(|o| o.borrow().as_deref() == Some(key.as_str()));
    let label = selected.as_ref().map(|s| s.to_string()).unwrap_or_default();
    let p_ = p();
    let k = key.clone();
    let head = div()
        .id(ui::uid(("select", &key)))
        .flex()
        .items_center()
        .gap(px(6.0))
        .w_full()
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(tokens::RADIUS_MD))
        .bg(p_.surface_sunken)
        .border_1()
        .border_color(if open { p_.accent } else { p_.border })
        .hover(move |s| s.border_color(p_.border_strong))
        .cursor_pointer()
        .child(line(label, tokens::BODY_SMALL, p_.ink).flex_1())
        .child(icon(Icon::ChevronDown, 14.0, p_.ink_muted))
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            OPEN_SELECT.with(|o| {
                let mut o = o.borrow_mut();
                *o = if o.as_deref() == Some(k.as_str()) { None } else { Some(k.clone()) };
            });
            window.refresh();
        });
    let mut root = div().relative().w_full().child(head);
    if open {
        let mut list = floating(col()).min_w(px(160.0)).max_h(px(280.0)).py(px(4.0));
        for o in options {
            let chosen = selected.as_ref() == Some(&o);
            let msg = on_select(o.clone());
            let item = div()
                .id(ui::uid(("select-opt", &key, o.to_string())))
                .px(px(10.0))
                .py(px(6.0))
                .cursor_pointer()
                .when_bg(chosen, p_.accent_tint)
                .hover(move |s| s.bg(p_.surface_sunken))
                .child(line(o.to_string(), tokens::BODY_SMALL, if chosen { p_.accent_deep } else { p_.ink }))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    OPEN_SELECT.with(|o| *o.borrow_mut() = None);
                    window.refresh();
                    ui::send(msg.clone(), cx);
                });
            list = list.child(item);
        }
        let list =
            list.id(ui::uid(("select-list", &key))).overflow_y_scroll().occlude().on_mouse_down_out(|_, window, _| {
                OPEN_SELECT.with(|o| *o.borrow_mut() = None);
                window.refresh();
            });
        root = root.child(deferred(anchored().child(div().mt(px(4.0)).child(list))).with_priority(2));
    }
    root.into_any_element()
}

trait WhenBg: Styled + Sized {
    fn when_bg(self, cond: bool, c: Color) -> Self {
        if cond { self.bg(c) } else { self }
    }
}

impl<T: Styled> WhenBg for T {}

/// A thin progress bar in the accent.
pub fn progress(value: f32, max: f32) -> El {
    let frac = if max > 0.0 { (value / max).clamp(0.0, 1.0) } else { 0.0 };
    div()
        .w_full()
        .h(px(6.0))
        .rounded(px(3.0))
        .bg(p().border)
        .child(div().h_full().w(gpui_kit::relative(frac)).rounded(px(3.0)).bg(p().accent))
        .into_any_element()
}

/// A 1 px line across: hairlines part a screen.
pub fn hairline() -> El {
    div().flex_none().w_full().h(px(1.0)).bg(p().border).into_any_element()
}

/// A short vertical divider between title bar groups.
pub fn divider() -> El {
    div().flex_none().w(px(1.0)).h(px(16.0)).bg(p().border).into_any_element()
}

fn strong_line(height: f32) -> Div {
    div().flex_none().w_full().h(px(height)).bg(p().border_strong)
}

/// 子持ち罫: a 2 px line over a 1 px line with 3 px between, under an app bar only.
pub fn rule() -> El {
    col().w_full().child(strong_line(2.0)).child(div().h(px(3.0))).child(strong_line(1.0)).into_any_element()
}

/// The kamon's frame around the one container being worked in: a 2 px band, a 3 px gap and a 1 px
/// line. `bg` fills inside the band.
pub fn frame(content: impl IntoElement, bg: Color, radius: f32) -> Div {
    let inner = div()
        .size_full()
        .border_1()
        .border_color(p().border_strong)
        .rounded(px((radius - 5.0).max(0.0)))
        .child(content);
    div().p(px(3.0)).bg(bg).border_2().border_color(p().border_strong).rounded(px(radius)).child(inner)
}

/// A dialog: a sheet in the kamon's frame over the dimmed window, with its title centred, a hairline
/// before the text and the actions as full-width buttons, the main one on top.
pub fn dialog(title: impl Into<String>, body: Option<El>, actions: Vec<El>) -> El {
    let mut sheet = col().w_full().child(
        div().w_full().flex().justify_center().child(t(title.into(), tokens::BODY, p().ink).medium().text_center()),
    );
    match body {
        Some(b) => {
            sheet = sheet
                .child(div().h(px(tokens::SPACE_3)))
                .child(hairline())
                .child(div().h(px(tokens::SPACE_3)))
                .child(b)
                .child(div().h(px(tokens::SPACE_4)));
        }
        None => sheet = sheet.child(div().h(px(tokens::SPACE_4))),
    }
    sheet = sheet.child(col().w_full().gap(px(6.0)).children(actions));
    let framed = frame(
        div().pt(px(tokens::SPACE_4)).px(px(tokens::SPACE_4)).pb(px(tokens::SPACE_3)).child(sheet),
        p().surface,
        tokens::RADIUS_LG,
    )
    .w_full()
    .max_w(px(384.0))
    .shadow(vec![shadow()]);
    div()
        .id("dialog-backdrop")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .p(px(20.0))
        .bg(alpha(p().ink, 0.3))
        .occlude()
        .child(framed)
        .into_any_element()
}

/// A dialog's full-width action button, its label centred.
pub fn dialog_button(label: impl Into<String>, variant: Variant, on: Option<Message>) -> El {
    button_box(plain(label.into(), tokens::LABEL), variant, Size::Lg.height(), tokens::SPACE_4, on)
        .w_full()
        .into_any_element()
}

/// A titled group of settings on a card, its rows parted by hairlines.
pub fn section(title: Option<String>, rows: Vec<El>) -> El {
    let mut c = col();
    if let Some(title) = title {
        c = c.child(div().pt(px(8.0)).pb(px(2.0)).child(line(title, tokens::CAPTION, p().ink_muted).medium()));
    }
    let n = rows.len();
    for (i, r) in rows.into_iter().enumerate() {
        c = c.child(r);
        if i + 1 < n {
            c = c.child(hairline());
        }
    }
    c.w_full()
        .py(px(6.0))
        .px(px(14.0))
        .bg(p().surface_raised)
        .border_1()
        .border_color(p().border)
        .rounded(px(tokens::RADIUS_LG))
        .into_any_element()
}

/// The loader icon, turning with the clock (the app ticks while one is shown).
pub fn spinner(size: f32, now: Instant) -> El {
    let turn = (now.duration_since(*START).as_millis() % 900) as f32 / 900.0;
    turned(Icon::LoaderCircle, size, p().ink_muted, turn).into_any_element()
}

static START: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

/// A rounded square of a project's colour (FR-64): 10 px, never a fill or a chip.
pub fn swatch(color: Color, size: f32) -> El {
    div().flex_none().size(px(size)).rounded(px(size * 0.3)).bg(color).into_any_element()
}

/// A plain clickable area with no look of its own (rows, the title bar's tags).
pub fn bare(content: impl IntoElement, on: Option<Message>, hover_bg: bool) -> Stateful<Div> {
    let mut b = clickable(on, "bare").rounded(px(tokens::RADIUS_MD)).child(content);
    if hover_bg {
        let sunk = p().surface_sunken;
        b = b.hover(move |s| s.bg(sunk));
    }
    b
}

/// Nothing (an empty slot in a row).
pub fn nothing() -> El {
    div().into_any_element()
}
