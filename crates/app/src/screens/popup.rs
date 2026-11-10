//! The app's own popup menu (a session's ⋯, コピー ▾, the camera's, a right click), drawn in the
//! window in Chidori's look: a white sheet with the `line` ring, rows that wash under the
//! pointer, a submenu beside its row. A menu taller than the window (the compact one, 380 × 170)
//! falls back to the native one, which may reach past the window (DECISIONS → UI, "Menus").
//!
//! A pick goes where a native menu's pick goes: `App::on_menu`, by its id.

use gpui_kit::{InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled, div, px};

use crate::app::{App, Message};
use crate::icons::{Icon, icon};
use crate::motion::{self, Look};
use crate::native::menu::Item;
use crate::paint::{Point, Size};
use crate::task::Task;
use crate::theme::{GRID, R_ROW, alpha, g, line, p, tokens, wash};
use crate::ui::{self, El, KeyPress};
use crate::widgets::{col, floating, row};

/// A row's height, a separator's, and the sheet's padding.
const ROW: f32 = 4.0 * GRID;
const SEPARATOR: f32 = GRID;
const PAD: f32 = 0.5 * GRID;
/// Kept clear of the window's edges.
const MARGIN: f32 = GRID;
const MIN_W: f32 = 20.0 * GRID;
const MAX_W: f32 = 36.0 * GRID;

#[derive(Debug, Clone)]
pub struct Open {
    pub items: Vec<Item>,
    /// Where it was asked for, in the window.
    pub at: Point,
    /// The submenu shown, by its row.
    pub sub: Option<usize>,
    /// The row the keyboard is on, in the submenu while one is entered.
    pub active: Option<usize>,
    pub in_sub: bool,
}

#[derive(Debug, Default)]
pub struct State {
    pub open: Option<Open>,
    /// What the rows of the last menu built by `Menu` send, by their `msg:<n>` id.
    pub msgs: Vec<Message>,
}

/// A menu whose rows send the app's own messages (a right click's menu, made of the same
/// actions as the buttons beside it), rather than ids a screen routes.
#[derive(Default)]
pub struct Menu {
    items: Vec<Item>,
    msgs: Vec<Message>,
}

impl Menu {
    pub fn new() -> Self {
        Self::default()
    }

    /// A row sending `msg`; greyed out without one.
    pub fn act(mut self, label: impl Into<String>, msg: Option<Message>) -> Self {
        let id = format!("msg:{}", self.msgs.len());
        let enabled = msg.is_some();
        self.msgs.push(msg.unwrap_or(Message::Noop));
        self.items.push(Item::action(id, label).enabled(enabled));
        self
    }

    /// A row with a check mark.
    pub fn check(mut self, label: impl Into<String>, checked: bool, msg: Message) -> Self {
        let id = format!("msg:{}", self.msgs.len());
        self.msgs.push(msg);
        self.items.push(Item::check(id, label, checked));
        self
    }

    pub fn separator(mut self) -> Self {
        if !self.items.is_empty() && !matches!(self.items.last(), Some(Item::Separator)) {
            self.items.push(Item::Separator);
        }
        self
    }

    /// Rows that route by id as any menu's do (a session's ⋯ items).
    pub fn items(mut self, items: Vec<Item>) -> Self {
        self.items.extend(items);
        self
    }

    /// The message that opens it at the pointer.
    pub fn open(self) -> Message {
        Message::PopupWith(self.items, self.msgs)
    }
}

/// The message a `msg:<n>` row of the last `Menu` sends.
pub fn picked(app: &App, n: &str) -> Task {
    n.parse::<usize>().ok().and_then(|n| app.menu.msgs.get(n).cloned()).map_or(Task::none(), Task::done)
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// The pointer is on a row of the top menu: its submenu opens, any other closes.
    Point(usize),
    Pick(String),
    Close,
}

fn m(msg: Msg) -> Message {
    Message::Menu(msg)
}

pub fn is_open(app: &App) -> bool {
    app.menu.open.is_some()
}

/// Opens a menu at `at`, or at the pointer. `false` when it can't fit the window, for the native
/// one to show instead.
pub fn open(app: &mut App, items: Vec<Item>, at: Option<Point>) -> bool {
    if items.is_empty() {
        return true;
    }
    if height(&items) + 2.0 * MARGIN > app.viewport.height {
        return false;
    }
    let at = at.unwrap_or_else(ui::pointer);
    app.menu.open = Some(Open { items, at, sub: None, active: None, in_sub: false });
    true
}

pub fn update(app: &mut App, msg: Msg) -> Task {
    match msg {
        Msg::Point(i) => {
            if let Some(o) = &mut app.menu.open {
                let has_sub = matches!(o.items.get(i), Some(Item::Submenu { enabled: true, .. }));
                o.sub = has_sub.then_some(i);
                o.active = Some(i);
                o.in_sub = false;
            }
            Task::none()
        }
        Msg::Pick(id) => {
            app.menu.open = None;
            Task::done(Message::Bridge(crate::bridge::UiEvent::Menu(id)))
        }
        Msg::Close => {
            app.menu.open = None;
            Task::none()
        }
    }
}

/// The rows the keyboard can land on.
fn live(items: &[Item]) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, it)| match it {
            Item::Action { enabled, .. } | Item::Check { enabled, .. } | Item::Submenu { enabled, .. } => *enabled,
            Item::Separator => false,
        })
        .map(|(i, _)| i)
        .collect()
}

fn step(items: &[Item], from: Option<usize>, down: bool) -> Option<usize> {
    let rows = live(items);
    if rows.is_empty() {
        return None;
    }
    let pos = from.and_then(|f| rows.iter().position(|&r| r == f));
    let next = match (pos, down) {
        (None, true) => 0,
        (None, false) => rows.len() - 1,
        (Some(p), true) => (p + 1) % rows.len(),
        (Some(p), false) => (p + rows.len() - 1) % rows.len(),
    };
    Some(rows[next])
}

fn id_of(item: &Item) -> Option<&str> {
    match item {
        Item::Action { id, enabled: true, .. } | Item::Check { id, enabled: true, .. } => Some(id),
        _ => None,
    }
}

/// Keys while a menu is open: ↑ ↓ to move, → into a submenu, ← out, Enter to pick, Esc to close.
/// The menu takes every key while it is open.
pub fn on_key(app: &mut App, key: &KeyPress) -> Task {
    let Some(o) = &mut app.menu.open else { return Task::none() };
    let sub_items = match o.sub.and_then(|s| o.items.get(s)) {
        Some(Item::Submenu { items, .. }) => Some(items.clone()),
        _ => None,
    };
    match key.code.as_str() {
        "Escape" if o.in_sub => {
            o.in_sub = false;
            o.active = o.sub;
        }
        "Escape" => app.menu.open = None,
        "down" | "up" => {
            let down = key.code == "down";
            if o.in_sub {
                o.active = sub_items.as_deref().and_then(|items| step(items, o.active, down));
            } else {
                o.active = step(&o.items, o.active, down);
                o.sub = None;
            }
        }
        "right" if !o.in_sub => {
            if let Some(a) = o.active
                && matches!(o.items.get(a), Some(Item::Submenu { enabled: true, .. }))
                && let Some(Item::Submenu { items, .. }) = o.items.get(a)
            {
                o.sub = Some(a);
                o.in_sub = true;
                o.active = step(items, None, true);
            }
        }
        "left" if o.in_sub => {
            o.in_sub = false;
            o.active = o.sub;
        }
        "Enter" | "Space" => {
            let items = if o.in_sub { sub_items.unwrap_or_default() } else { o.items.clone() };
            let Some(a) = o.active else { return Task::none() };
            match items.get(a) {
                Some(Item::Submenu { enabled: true, items, .. }) if !o.in_sub => {
                    o.sub = Some(a);
                    o.in_sub = true;
                    o.active = step(items, None, true);
                }
                Some(item) => {
                    if let Some(id) = id_of(item) {
                        return update(app, Msg::Pick(id.to_string()));
                    }
                }
                None => {}
            }
        }
        _ => {}
    }
    Task::none()
}

fn height(items: &[Item]) -> f32 {
    2.0 * PAD + items.iter().map(|i| if matches!(i, Item::Separator) { SEPARATOR } else { ROW }).sum::<f32>()
}

/// Roughly how wide a label draws: CJK at the body size's full width, the rest at about half.
fn text_width(s: &str) -> f32 {
    s.chars().map(|c| if c.is_ascii() { 7.5 } else { tokens::BODY.size }).sum()
}

fn width(items: &[Item]) -> f32 {
    let checks = items.iter().any(|i| matches!(i, Item::Check { .. }));
    let widest = items
        .iter()
        .map(|i| match i {
            Item::Action { label, .. } | Item::Check { label, .. } => text_width(label),
            Item::Submenu { label, .. } => text_width(label) + 3.0 * GRID,
            Item::Separator => 0.0,
        })
        .fold(0.0, f32::max);
    let check = if checks { 3.0 * GRID } else { 0.0 };
    (widest + check + 4.0 * GRID + 2.0 * PAD).clamp(MIN_W, MAX_W)
}

/// Where a sheet of `size` goes so it stays in the window: below and right of `at`, flipped up
/// or left where it would run past an edge.
fn place(at: Point, size: Size, window: Size, flip_x: Option<f32>) -> Point {
    let mut x = at.x;
    if x + size.width > window.width - MARGIN {
        x = flip_x.unwrap_or(at.x - size.width);
    }
    let mut y = at.y;
    if y + size.height > window.height - MARGIN {
        y = at.y - size.height;
    }
    Point::new(
        x.clamp(MARGIN, (window.width - MARGIN - size.width).max(MARGIN)),
        y.clamp(MARGIN, (window.height - MARGIN - size.height).max(MARGIN)),
    )
}

fn item_row(item: &Item, index: usize, active: bool, checks: bool, top: bool, open_sub: bool) -> El {
    let p_ = p();
    let (label, enabled, checked) = match item {
        Item::Action { label, enabled, .. } | Item::Submenu { label, enabled, .. } => (label.clone(), *enabled, false),
        Item::Check { label, enabled, checked, .. } => (label.clone(), *enabled, *checked),
        Item::Separator => {
            return div()
                .flex()
                .items_center()
                .h(px(SEPARATOR))
                .px(g(1.0))
                .child(div().w_full().h(px(1.0)).bg(p_.line))
                .into_any_element();
        }
    };
    let color = if enabled { p_.ink } else { alpha(p_.muted, 0.6) };
    let mut r =
        row().id(ui::uid(("menu-row", top, index, &label))).gap(g(1.0)).h(px(ROW)).px(g(1.5)).rounded(px(R_ROW - PAD));
    if checks {
        r = r.child(if checked {
            icon(Icon::Check, 16.0, color).into_any_element()
        } else {
            div().flex_none().size(px(16.0)).into_any_element()
        });
    }
    r = r.child(line(label, tokens::BODY, color).flex_1().min_w_0());
    if matches!(item, Item::Submenu { .. }) {
        r = r.child(icon(Icon::ChevronRight, 16.0, if enabled { p_.muted } else { color }));
    }
    let wash = wash();
    if active || open_sub {
        r = r.bg(wash);
    }
    if !enabled {
        return r.into_any_element();
    }
    r = r.cursor_pointer().hover(move |s| s.bg(wash));
    if top {
        r = r.on_hover(move |hovered, _, cx| {
            if *hovered {
                ui::send(m(Msg::Point(index)), cx);
            }
        });
    }
    if let Some(id) = id_of(item) {
        let id = id.to_string();
        r = r.on_click(move |_, _, cx| {
            cx.stop_propagation();
            ui::send(m(Msg::Pick(id.clone())), cx);
        });
    } else {
        // A submenu's row opens it on a click as well as on the pointer.
        r = r.on_click(move |_, _, cx| {
            cx.stop_propagation();
            ui::send(m(Msg::Point(index)), cx);
        });
    }
    r.into_any_element()
}

fn sheet(key: &str, items: &[Item], at: Point, w: f32, rows: Vec<El>) -> El {
    let h = height(items);
    let body = floating(col().id(ui::uid(("menu-sheet", key))).occlude())
        .w(px(w))
        .h(px(h))
        .p(px(PAD - 2.0))
        .children(rows)
        // A click on the sheet's own padding stays in the menu.
        .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(gpui_kit::MouseButton::Right, |_, _, cx| cx.stop_propagation());
    let p_ = p();
    let sheet = motion::enter(
        format!("menu-{key}"),
        body,
        Some(motion::From { dx: 0.0, dy: -0.5 * GRID }),
        false,
        Look { color: p_.surface, side: p_.side, radius: R_ROW },
    );
    div().absolute().left(px(at.x)).top(px(at.y)).child(sheet).into_any_element()
}

/// The open menu over the whole window, or nothing. A press anywhere outside closes it.
pub fn view(app: &App) -> Option<El> {
    let o = app.menu.open.as_ref()?;
    let window = app.viewport;
    let w = width(&o.items);
    let h = height(&o.items);
    let at = place(o.at, Size::new(w, h), window, None);
    let checks = o.items.iter().any(|i| matches!(i, Item::Check { .. }));
    let rows = o
        .items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let active = !o.in_sub && o.active == Some(i);
            item_row(it, i, active, checks, true, o.sub == Some(i))
        })
        .collect();
    let mut layer = div()
        .id("menu-layer")
        .absolute()
        .inset_0()
        .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
            ui::send(m(Msg::Close), cx);
        })
        .on_mouse_down(gpui_kit::MouseButton::Right, |_, _, cx| {
            cx.stop_propagation();
            ui::send(m(Msg::Close), cx);
        })
        .occlude()
        .child(sheet("top", &o.items, at, w, rows));
    if let Some(s) = o.sub
        && let Some(Item::Submenu { items, .. }) = o.items.get(s)
    {
        let sw = width(items);
        let sh = height(items);
        let row_top = at.y + PAD + height(&o.items[..s]) - 2.0 * PAD;
        // Beside its row, on the left where the right has no room.
        let beside = Point::new(at.x + w - PAD, row_top - PAD);
        let sub_at = place(beside, Size::new(sw, sh), window, Some(at.x - sw + PAD));
        let checks = items.iter().any(|i| matches!(i, Item::Check { .. }));
        let rows = items
            .iter()
            .enumerate()
            .map(|(i, it)| item_row(it, i, o.in_sub && o.active == Some(i), checks, false, false))
            .collect();
        layer = layer.child(sheet(&format!("sub-{s}"), items, sub_at, sw, rows));
    }
    Some(layer.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<Item> {
        vec![
            Item::action("a:1", "コピー"),
            Item::Separator,
            Item::action("a:2", "削除").enabled(false),
            Item::submenu("移動", vec![Item::action("a:3", "x")]),
        ]
    }

    #[test]
    fn keys_skip_separators_and_disabled_rows() {
        let it = items();
        assert_eq!(step(&it, None, true), Some(0));
        assert_eq!(step(&it, Some(0), true), Some(3));
        assert_eq!(step(&it, Some(3), true), Some(0));
        assert_eq!(step(&it, Some(0), false), Some(3));
    }

    #[test]
    fn a_sheet_stays_in_the_window() {
        let win = Size::new(420.0, 640.0);
        let s = Size::new(200.0, 300.0);
        // Room below and right: where it was asked for.
        assert_eq!(place(Point::new(50.0, 60.0), s, win, None), Point::new(50.0, 60.0));
        // Near the bottom right: flipped up and left.
        let p = place(Point::new(400.0, 600.0), s, win, None);
        assert_eq!(p, Point::new(200.0, 300.0));
        // Never past the margin, even when asked from outside.
        let p = place(Point::new(-50.0, -50.0), s, win, None);
        assert_eq!(p, Point::new(MARGIN, MARGIN));
    }

    #[test]
    fn height_counts_rows_and_separators() {
        assert_eq!(height(&items()), 2.0 * PAD + 3.0 * ROW + SEPARATOR);
    }
}
