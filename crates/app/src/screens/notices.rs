//! Banners at the top with one action (section 5), toasts that come in from the bottom edge, and
//! what shows while a file is held over the window. The palette has no red or yellow tint for
//! them: an error is an ink banner with a coral icon, a warning a white one with a grey icon, and
//! each says what happened in words, so none relies on colour alone.

use std::time::Duration;

use gpui_kit::{IntoElement, ParentElement, Styled, div, px};

use crate::app::{App, Level, Message, TOAST_FOR};
use crate::i18n::{t, t_json};
use crate::icons::{Icon, icon};
use crate::motion::{self, Look};
use crate::theme::{GRID, Ink, R_ROW, g, line, p, t as text, tokens};
use crate::ui::{El, clickable};
use crate::widgets::{Size, Variant, btn, col, nothing, row};

/// A toast starts leaving this long before it goes.
const LEAVE_FOR: Duration = Duration::from_millis(400);

pub fn banners(app: &App) -> El {
    if app.banners.is_empty() {
        return nothing();
    }
    let rows = app.banners.iter().map(|b| {
        let n = &b.notice;
        let level: Level = n.level.into();
        let p_ = p();
        let (glyph, bg, fg, icon_tone) = match level {
            Level::Error => (Icon::CircleAlert, p_.ink, p_.on_ink, p_.them_wave),
            Level::Warn => (Icon::TriangleAlert, p_.surface, p_.ink, p_.muted),
            Level::Info => (Icon::Info, p_.surface, p_.ink, p_.muted),
        };
        let mut r = row()
            .gap(g(1.0))
            .pl(g(2.0))
            .pr(g(1.0))
            .py(g(1.0))
            .w_full()
            .rounded(px(R_ROW))
            .bg(bg)
            .child(icon(glyph, 16.0, icon_tone))
            .child(text(t_json(&n.message, &n.params), tokens::SMALL, fg).flex_1().min_w_0());
        if let Some(action) = &n.action {
            let command = action.command.clone();
            let variant = if level == Level::Error { Variant::Secondary } else { Variant::Primary };
            r = r.child(btn(t(&action.label), variant, Size::Sm, Some(Message::NoticeAction(command, Some(b.id)))));
        }
        let dismiss = clickable(Some(Message::DismissBanner(b.id)), "dismiss")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(g(4.0))
            .rounded(g(2.0))
            .child(icon(Icon::X, 16.0, if level == Level::Error { p_.faint } else { p_.muted }));
        r.child(dismiss)
    });
    col().flex_none().gap(g(1.0)).px(g(2.0)).pb(g(1.0)).children(rows).into_any_element()
}

/// In-app toasts, shown for 2 s at the bottom: ink pills that come in from the bottom edge and go
/// out the same way, their icon telling the level.
pub fn toasts(app: &App) -> El {
    if app.toasts.is_empty() {
        return nothing();
    }
    let p_ = p();
    let items = app.toasts.iter().map(|toast| {
        let (glyph, tone) = match toast.level {
            Level::Error => (Icon::CircleAlert, p_.them_wave),
            Level::Warn => (Icon::TriangleAlert, p_.on_ink),
            Level::Info => (Icon::CircleCheckBig, p_.on_ink),
        };
        let pill = row()
            .gap(g(1.0))
            .h(g(5.0))
            .px(g(2.0))
            .rounded(g(2.5))
            .bg(p_.ink)
            .child(icon(glyph, 16.0, tone))
            .child(line(toast.text.clone(), tokens::LABEL, p_.on_ink).bold());
        let leaving = app.now.saturating_duration_since(toast.at) + LEAVE_FOR >= TOAST_FOR;
        let key = format!("toast-{:?}", toast.at);
        let from = Some(motion::From { dx: 0.0, dy: 12.0 * GRID });
        motion::enter(key, pill, from, leaving, Look { color: p_.ink, side: p_.ink_side, radius: 2.5 * GRID })
    });
    // Over the bar at the bottom, so they never cover the transcript's newest line for long.
    col()
        .absolute()
        .bottom(g(11.0))
        .left(g(2.0))
        .right(g(2.0))
        .items_center()
        .gap(g(1.0))
        .children(items)
        .into_any_element()
}

/// While a file is held over the window (FR-18).
pub fn drop_overlay() -> El {
    let body = col()
        .items_center()
        .gap(g(2.0))
        .child(icon(Icon::FileAudio, 24.0, p().muted))
        .child(line(t("dropToTranscribe"), tokens::BODY, p().ink).bold());
    div()
        .absolute()
        .top(g(6.0))
        .left_0()
        .right_0()
        .bottom_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(p().ground)
        .child(body)
        .into_any_element()
}
