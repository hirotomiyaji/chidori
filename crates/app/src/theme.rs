//! Chidori's look (DECISIONS → UI): greyscale chrome, colour only for who is speaking (相手 coral,
//! 自分 teal) and that it is recording (one red dot), plain rounded rects with no shadows, glows or
//! gradients, everything on the 8 px grid. Views read the palette when they are built, and the
//! window is drawn again when Windows switches between light and dark, so a switch restyles
//! everything.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::{Div, FontWeight, Hsla, ParentElement, Pixels, Rgba, SharedString, Styled, div, px};

/// A colour (sRGB, 0..1), turned into GPUI's where it is drawn.
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

    /// A colour from `0xRRGGBB`.
    pub const fn hex(v: u32) -> Color {
        Color {
            r: ((v >> 16) & 0xff) as f32 / 255.0,
            g: ((v >> 8) & 0xff) as f32 / 255.0,
            b: (v & 0xff) as f32 / 255.0,
            a: 1.0,
        }
    }
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

/// The colours, by what they are for.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// The window's background.
    pub ground: Color,
    /// Panels, bubbles, the active tab's pill, white buttons.
    pub surface: Color,
    /// Text, the recording card, primary buttons, switches that are on.
    pub ink: Color,
    /// Text on ink, and the secondary text there.
    pub on_ink: Color,
    /// Secondary text, icons, inactive tabs.
    pub muted: Color,
    /// Provisional (grey) lines.
    pub faint: Color,
    /// Switches that are off, the provisional line's ring, small dividers.
    pub line: Color,
    /// Times above bubbles.
    pub meta: Color,
    /// 相手: the name, and its half of the line on a panel and on the ink card.
    pub them: Color,
    pub them_wave: Color,
    /// 自分, the same three.
    pub me: Color,
    pub me_wave: Color,
    /// The record dot.
    pub rec: Color,
    /// A row under the pointer on a panel (buttons use `hover_of` and `wash`).
    pub hover_panel: Color,
    /// The jello's side under a white thing (pill, chip, bubble, panel), a switch's thumb, and
    /// the ink card.
    pub side: Color,
    pub thumb_side: Color,
    pub ink_side: Color,
    /// Behind a dialog.
    pub scrim: Color,
}

pub const LIGHT: Palette = Palette {
    ground: Color::hex(0xF2F2F0),
    surface: Color::hex(0xFFFFFF),
    ink: Color::hex(0x1C1C1A),
    on_ink: Color::hex(0xFFFFFF),
    muted: Color::hex(0x72726C),
    faint: Color::hex(0xA3A39C),
    line: Color::hex(0xE3E3DF),
    meta: Color::hex(0x8A8A84),
    them: Color::hex(0xD2432F),
    them_wave: Color::hex(0xF58A78),
    me: Color::hex(0x0B7A6A),
    me_wave: Color::hex(0x4FC7B3),
    rec: Color::hex(0xF05A47),
    hover_panel: Color::hex(0xF2F2F0),
    side: Color::hex(0xD6D6D1),
    thumb_side: Color::hex(0xBDBDB8),
    ink_side: Color::hex(0x000000),
    scrim: Color { r: 0.11, g: 0.11, b: 0.10, a: 0.3 },
};

/// Derived the same way (DECISIONS → UI, "Dark"): a near-black ground, surfaces a step lighter,
/// and the ink card turned light; coral, teal and red stay the only hues.
pub const DARK: Palette = Palette {
    ground: Color::hex(0x141413),
    surface: Color::hex(0x1F1F1D),
    ink: Color::hex(0xECECE8),
    on_ink: Color::hex(0x1C1C1A),
    muted: Color::hex(0xA3A39C),
    faint: Color::hex(0x72726C),
    line: Color::hex(0x33332F),
    meta: Color::hex(0x8A8A84),
    them: Color::hex(0xF58A78),
    them_wave: Color::hex(0xF58A78),
    me: Color::hex(0x4FC7B3),
    me_wave: Color::hex(0x4FC7B3),
    rec: Color::hex(0xF05A47),
    hover_panel: Color::hex(0x292927),
    side: Color::hex(0x0B0B0A),
    thumb_side: Color::hex(0x8A8A84),
    ink_side: Color::hex(0xBDBDB8),
    scrim: Color { r: 0.0, g: 0.0, b: 0.0, a: 0.5 },
};

/// The grid every size at rest sits on (Jello).
pub const GRID: f32 = 8.0;

/// `n` grid cells.
pub fn g(n: f32) -> Pixels {
    px(n * GRID)
}

/// Corner radii: panels and the recording card, rows and chips, a bubble's corner on its
/// speaker's side, the ON AIR tag.
pub const R_PANEL: f32 = 3.0 * GRID;
pub const R_ROW: f32 = 2.0 * GRID;
pub const R_TAG: f32 = GRID;
pub const R_NOTCH: f32 = 4.0;

#[derive(Debug, Clone, Copy)]
pub struct TypeStyle {
    pub size: f32,
    pub line_height: f32,
    pub bold: bool,
    pub mono: bool,
}

/// The type scale: one family at 14/24 for text, 12/16 small, 11/24 for the times above bubbles,
/// 13 for tabs and buttons, and the timer in mono.
pub mod tokens {
    use super::{GRID, TypeStyle};

    pub const BODY: TypeStyle = TypeStyle { size: 14.0, line_height: 3.0 * GRID, bold: false, mono: false };
    pub const SMALL: TypeStyle = TypeStyle { size: 12.0, line_height: 2.0 * GRID, bold: false, mono: false };
    pub const META: TypeStyle = TypeStyle { size: 11.0, line_height: 3.0 * GRID, bold: false, mono: false };
    pub const LABEL: TypeStyle = TypeStyle { size: 13.0, line_height: 3.0 * GRID, bold: false, mono: false };
    pub const HEADING: TypeStyle = TypeStyle { size: 16.0, line_height: 3.0 * GRID, bold: true, mono: false };
    pub const TITLE: TypeStyle = TypeStyle { size: 20.0, line_height: 4.0 * GRID, bold: true, mono: false };
    pub const TIMER: TypeStyle = TypeStyle { size: 20.0, line_height: 3.0 * GRID, bold: false, mono: true };
    pub const TIMER_SMALL: TypeStyle = TypeStyle { size: 16.0, line_height: 3.0 * GRID, bold: false, mono: true };
}

pub const SANS: &str = "Zen Kaku Gothic New";
pub const MONO: &str = "IBM Plex Mono";

pub const FONTS: [&[u8]; 3] = [
    include_bytes!("../brand/fonts/ZenKakuGothicNew-Regular.ttf"),
    include_bytes!("../brand/fonts/ZenKakuGothicNew-Bold.ttf"),
    include_bytes!("../brand/fonts/IBMPlexMono-Regular.ttf"),
];

static DARK_MODE: AtomicBool = AtomicBool::new(false);

pub fn set_dark(dark: bool) {
    DARK_MODE.store(dark, Ordering::Relaxed);
}

pub fn is_dark() -> bool {
    DARK_MODE.load(Ordering::Relaxed)
}

/// The palette in use: light or dark, following Windows.
pub fn p() -> &'static Palette {
    if is_dark() { &DARK } else { &LIGHT }
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

/// A filled button's ground under the pointer: one step from its own ground, toward its text on
/// an ink pill and toward ink on any other, so it shows on the ground, a panel or a dialog alike
/// (DECISIONS → UI, "Hover").
pub fn hover_of(bg: Color) -> Color {
    let p = p();
    if bg == p.ink { mix(p.on_ink, p.ink, 0.15) } else { mix(p.ink, bg, 0.10) }
}

/// What a button with no ground of its own (an icon, a ghost) lays under the pointer: ink, thin
/// enough to darken (or, in dark, lighten) whatever it sits on, a hovered row included.
pub fn wash() -> Color {
    alpha(p().ink, if is_dark() { 0.10 } else { 0.07 })
}

/// Text styles on any styled element: bold and the mono face.
pub trait Ink: Styled + Sized {
    fn bold(self) -> Self {
        self.font_weight(FontWeight::BOLD)
    }

    fn mono(self) -> Self {
        self.font_family(MONO)
    }

    /// One of the type styles, in a colour.
    fn type_style(self, style: TypeStyle, color: Color) -> Self {
        let s = self.text_size(px(style.size)).line_height(px(style.line_height)).text_color(color);
        let s = if style.mono { s.font_family(MONO) } else { s };
        if style.bold { s.font_weight(FontWeight::BOLD) } else { s }
    }
}

impl<T: Styled> Ink for T {}

/// Text in one of the type styles; it wraps.
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
    if style.bold { d.font_weight(FontWeight::BOLD) } else { d }
}

/// Project colours (FR-64): small dots only, never a fill.
pub fn project_color(name: &str) -> Color {
    let dark = is_dark();
    let hex = match (name, dark) {
        ("blue", false) => 0x3C6FD8,
        ("blue", true) => 0x7FA2EA,
        ("teal", false) => 0x2A8C96,
        ("teal", true) => 0x6CC0C8,
        ("green", false) => 0x2E9E6B,
        ("green", true) => 0x6CC79C,
        ("amber", false) => 0xC98A1B,
        ("amber", true) => 0xE2B35E,
        ("violet", false) => 0x8A5CC2,
        ("violet", true) => 0xB596DE,
        ("brown", false) => 0x9A6B4F,
        ("brown", true) => 0xC79D84,
        _ => return p().muted,
    };
    Color::hex(hex)
}

pub const PROJECT_COLORS: [&str; 7] = ["slate", "blue", "teal", "green", "amber", "violet", "brown"];

/// Keeps GPUI Kit's own components (the text boxes, tooltips, scrollbars) in the app's colours and
/// type. Called at startup and whenever Windows switches between light and dark.
pub fn apply_to_kit(cx: &mut gpui_kit::App) {
    use gpui_kit::component::{Theme, ThemeMode};
    let mode = if is_dark() { ThemeMode::Dark } else { ThemeMode::Light };
    Theme::change(mode, None, cx);
    let p = p();
    Theme::update(cx, |th| {
        let c = |c: Color| -> Hsla { c.into() };
        th.font_family = SANS.into();
        th.font_size = px(tokens::BODY.size);
        th.mono_font_family = MONO.into();
        th.radius = px(R_ROW);
        th.radius_lg = px(R_PANEL);
        th.colors.background = c(p.surface);
        th.colors.foreground = c(p.ink);
        // Text boxes sit on white panels as ground-coloured pills, with no edge of their own.
        th.colors.border = c(p.line);
        th.colors.input = c(p.line);
        th.colors.ring = c(p.ink);
        th.colors.caret = c(p.ink);
        th.colors.selection = c(alpha(p.me_wave, 0.35));
        th.colors.primary = c(p.ink);
        th.colors.primary_hover = c(p.ink);
        th.colors.primary_foreground = c(p.on_ink);
        th.colors.muted = c(p.ground);
        th.colors.muted_foreground = c(p.muted);
        th.colors.accent = c(p.hover_panel);
        th.colors.accent_foreground = c(p.ink);
        th.colors.popover = c(p.surface);
        th.colors.popover_foreground = c(p.ink);
        th.colors.scrollbar = c(Color::TRANSPARENT);
        th.colors.scrollbar_thumb = c(p.line);
        th.colors.scrollbar_thumb_hover = c(p.faint);
        th.colors.danger = c(p.them);
        th.colors.warning = c(p.ink);
        th.colors.success = c(p.me);
    });
}
