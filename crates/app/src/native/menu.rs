//! Native popup menus (a session's ⋯, コピー ▾, the camera's menu...). Native rather than drawn,
//! because the compact window (380 × 170) is too small to hold a dropdown, and a native menu may
//! reach past the window. A pick arrives through the bridge as `UiEvent::Menu(id)`.

use tray_icon::menu::{CheckMenuItem, ContextMenu, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

use crate::bridge::{self, UiEvent};

#[derive(Debug, Clone)]
pub enum Item {
    Action {
        id: String,
        label: String,
        enabled: bool,
    },
    /// An item with a check mark, shown checked or not.
    Check {
        id: String,
        label: String,
        checked: bool,
        enabled: bool,
    },
    Separator,
    Submenu {
        label: String,
        items: Vec<Item>,
        enabled: bool,
    },
}

impl Item {
    pub fn action(id: impl Into<String>, label: impl Into<String>) -> Self {
        Item::Action { id: id.into(), label: label.into(), enabled: true }
    }

    pub fn check(id: impl Into<String>, label: impl Into<String>, checked: bool) -> Self {
        Item::Check { id: id.into(), label: label.into(), checked, enabled: true }
    }

    pub fn submenu(label: impl Into<String>, items: Vec<Item>) -> Self {
        Item::Submenu { label: label.into(), items, enabled: true }
    }

    /// The same item, greyed out unless `enabled`.
    pub fn enabled(mut self, on: bool) -> Self {
        match &mut self {
            Item::Action { enabled, .. } | Item::Check { enabled, .. } | Item::Submenu { enabled, .. } => *enabled = on,
            Item::Separator => {}
        }
        self
    }
}

/// Sends menu picks into the bridge. Call once at startup.
pub fn init() {
    MenuEvent::set_event_handler(Some(|event: MenuEvent| {
        bridge::send(UiEvent::Menu(event.id.0));
    }));
}

fn build(items: &[Item]) -> Vec<Box<dyn IsMenuItem>> {
    items
        .iter()
        .map(|item| -> Box<dyn IsMenuItem> {
            match item {
                Item::Action { id, label, enabled } => Box::new(MenuItem::with_id(id.as_str(), label, *enabled, None)),
                Item::Check { id, label, checked, enabled } => {
                    Box::new(CheckMenuItem::with_id(id.as_str(), label, *enabled, *checked, None))
                }
                Item::Separator => Box::new(PredefinedMenuItem::separator()),
                Item::Submenu { label, items, enabled } => {
                    let children = build(items);
                    let refs: Vec<&dyn IsMenuItem> = children.iter().map(|c| c.as_ref()).collect();
                    let sub = Submenu::new(label, *enabled);
                    if let Err(e) = sub.append_items(&refs) {
                        tracing::warn!("submenu: {e}");
                    }
                    Box::new(sub)
                }
            }
        })
        .collect()
}

/// Shows a menu at a point in the window (logical pixels), or at the cursor. Blocks until the
/// menu closes; the pick, if any, follows as an event.
pub fn popup(hwnd: u64, items: &[Item], at: Option<crate::paint::Point>) {
    let menu = Menu::new();
    let children = build(items);
    let refs: Vec<&dyn IsMenuItem> = children.iter().map(|c| c.as_ref()).collect();
    if let Err(e) = menu.append_items(&refs) {
        tracing::warn!("popup menu: {e}");
        return;
    }
    let position =
        at.map(|p| tray_icon::dpi::Position::Logical(tray_icon::dpi::LogicalPosition::new(p.x as f64, p.y as f64)));
    #[cfg(windows)]
    // SAFETY: `hwnd` is the app's own live window, from GPUI.
    unsafe {
        let picked = menu.show_context_menu_for_hwnd(hwnd as isize, position);
        tracing::debug!("popup menu closed, picked: {picked}");
    }
    #[cfg(not(windows))]
    let _ = (hwnd, position);
}
