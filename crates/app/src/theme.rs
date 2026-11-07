//! The kamon brand in GPUI: tokens from `brand/tokens.json`, the two fonts, and the palette in use.
//! Views read the palette when they are built, and the window is drawn again when Windows switches
//! between light and dark, so a switch restyles everything.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::{Div, FontWeight, Hsla, ParentElement, Rgba, SharedString, Styled, div, px};

/// A colour in the brand's terms (sRGB, 0..1), turned into GPUI's where it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
}

impl From<Color> for Hsla {
    fn from(c: Color) -> Self {
        Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into()
    }
}

impl From<Color> for gpui_kit::Background {
    fn from(c: Color) -> Self {
        Hsla::from(c).into()
    }
}

impl From<Color> for gpui_kit::Fill {
    fn from(c: Color) -> Self {
        gpui_kit::Fill::Color(Hsla::from(c).into())
    }
}

// The brand's whole token set is generated, used or not, so the app reads like the kit.
#[allow(dead_code)]
pub mod tokens {
    use super::Color;
    include!(concat!(env!("OUT_DIR"), "/tokens.rs"));
}

pub use tokens::{Palette, TypeStyle};

pub const SANS: &str = "Zen Kaku Gothic New";
pub const MONO: &str = "IBM Plex Mono";

pub const FONTS: [&[u8]; 3] = [
    include_bytes!("../brand/fonts/ZenKakuGothicNew-Regular.ttf"),
    include_bytes!("../brand/fonts/ZenKakuGothicNew-Medium.ttf"),
    include_bytes!("../brand/fonts/IBMPlexMono-Regular.ttf"),
];

static DARK: AtomicBool = AtomicBool::new(false);

pub fn set_dark(dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
}

pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

/// The palette in use: light or dark, following Windows.
pub fn p() -> &'static Palette {
    if is_dark() { &tokens::DARK } else { &tokens::LIGHT }
}

/// `a` over `b`, `amount` of the way (CSS `color-mix`).
pub fn mix(a: Color, b: Color, amount: f32) -> Color {
    Color {
        r: a.r * amount + b.r * (1.0 - amount),
        g: a.g * amount + b.g * (1.0 - amount),
        b: a.b * amount + b.b * (1.0 - amount),
        a: 1.0,
    }
}

pub fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// The hint under a finished session's line: the accent's tint, faded into the ground.
pub fn hint() -> Color {
    mix(p().accent_tint, p().surface, 0.45)
}

/// Text styles on any styled element: the medium weight and the mono face.
pub trait Ink: Styled + Sized {
    fn medium(self) -> Self {
        self.font_weight(FontWeight::MEDIUM)
    }

    fn mono(self) -> Self {
        self.font_family(MONO)
    }

    /// One of the brand's type styles, in a colour.
    fn type_style(self, style: TypeStyle, color: Color) -> Self {
        let s = self.text_size(px(style.size)).line_height(px(style.line_height)).text_color(color);
        let s = if style.mono { s.font_family(MONO) } else { s };
        if style.medium { s.font_weight(FontWeight::MEDIUM) } else { s }
    }
}

impl<T: Styled> Ink for T {}

/// Text in one of the brand's type styles; it wraps.
pub fn t(s: impl Into<SharedString>, style: TypeStyle, color: Color) -> Div {
    div().type_style(style, color).child(s.into())
}

/// One line that never wraps (labels, buttons, the title bar); what doesn't fit ends in `…`.
pub fn line(s: impl Into<SharedString>, style: TypeStyle, color: Color) -> Div {
    t(s, style, color).whitespace_nowrap().overflow_hidden().text_ellipsis()
}

/// One line in the colour of what holds it (a button's label follows the button's state).
pub fn plain(s: impl Into<SharedString>, style: TypeStyle) -> Div {
    let d = div().text_size(px(style.size)).line_height(px(style.line_height)).whitespace_nowrap().child(s.into());
    let d = if style.mono { d.font_family(MONO) } else { d };
    if style.medium { d.font_weight(FontWeight::MEDIUM) } else { d }
}

/// Project colours (FR-64): small squares only, from the brand's palette, never the accent.
pub fn project_color(name: &str) -> Color {
    let p = p();
    match name {
        "blue" => p.ai,
        "teal" => p.tetsu,
        "green" => p.moss,
        "amber" => p.warning,
        "violet" => p.budo,
        "brown" => p.blush,
        _ => p.ink_subtle,
    }
}

pub const PROJECT_COLORS: [&str; 7] = ["slate", "blue", "teal", "green", "amber", "violet", "brown"];

/// Keeps GPUI Kit's own components (the text boxes, tooltips, scrollbars) in the brand's colours
/// and type. Called at startup and whenever Windows switches between light and dark.
pub fn apply_to_kit(cx: &mut gpui_kit::App) {
    use gpui_kit::component::{Theme, ThemeMode};
    let mode = if is_dark() { ThemeMode::Dark } else { ThemeMode::Light };
    Theme::change(mode, None, cx);
    let p = p();
    Theme::update(cx, |th| {
        let c = |c: Color| -> Hsla { c.into() };
        th.font_family = SANS.into();
        th.font_size = px(tokens::BODY_SMALL.size);
        th.mono_font_family = MONO.into();
        th.radius = px(tokens::RADIUS_MD);
        th.radius_lg = px(tokens::RADIUS_LG);
        th.colors.background = c(p.surface);
        th.colors.foreground = c(p.ink);
        th.colors.border = c(p.border);
        th.colors.input = c(p.border);
        th.colors.ring = c(p.accent);
        th.colors.caret = c(p.ink);
        th.colors.selection = c(p.accent_tint);
        th.colors.primary = c(p.accent);
        th.colors.primary_hover = c(p.accent_hover);
        th.colors.primary_foreground = c(p.on_accent);
        th.colors.muted = c(p.surface_sunken);
        th.colors.muted_foreground = c(p.ink_muted);
        th.colors.accent = c(p.surface_sunken);
        th.colors.accent_foreground = c(p.ink);
        th.colors.popover = c(p.surface_raised);
        th.colors.popover_foreground = c(p.ink);
        th.colors.scrollbar = c(Color::TRANSPARENT);
        th.colors.scrollbar_thumb = c(p.border_strong);
        th.colors.scrollbar_thumb_hover = c(p.ink_muted);
        th.colors.danger = c(p.danger);
        th.colors.warning = c(p.warning);
        th.colors.success = c(p.success);
    });
}
