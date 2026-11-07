//! Banners at the top with one action (section 5), toasts under the title bar, and what shows
//! while a file is held over the window.

use gpui_kit::{IntoElement, ParentElement, Styled, div, px};

use crate::app::{App, Level, Message};
use crate::i18n::{t, t_json};
use crate::icons::{Icon, icon};
use crate::theme::{Ink, line, p, t as text, tokens};
use crate::ui::{El, clickable};
use crate::widgets::{Size, Variant, btn_with, col, floating, nothing, row};

/// Errors on the brand's brick tint and warnings on its ochre, each with its icon and words, so
/// none relies on colour alone: the accent is near both.
pub fn banners(app: &App) -> El {
    if app.banners.is_empty() {
        return nothing();
    }
    let rows = app.banners.iter().map(|b| {
        let n = &b.notice;
        let level: Level = n.level.into();
        let (glyph, tone, icon_tone) = match level {
            Level::Error => (Icon::CircleAlert, p().danger_tint, p().danger),
            Level::Warn => (Icon::TriangleAlert, p().warning_tint, p().warning),
            Level::Info => (Icon::Info, p().surface_sunken, p().ink_muted),
        };
        let mut r = row()
            .gap(px(10.0))
            .px(px(12.0))
            .py(px(8.0))
            .w_full()
            .rounded(px(tokens::RADIUS_LG))
            .bg(tone)
            .child(icon(glyph, 15.0, icon_tone))
            .child(text(t_json(&n.message, &n.params), tokens::CAPTION, p().ink).flex_1().min_w_0());
        if let Some(action) = &n.action {
            let command = action.command.clone();
            r = r.child(btn_with(
                line(t(&action.label), tokens::CAPTION, p().ink).medium(),
                Variant::Secondary,
                Size::Sm,
                Some(Message::NoticeAction(command, Some(b.id))),
            ));
        }
        r.child(clickable(Some(Message::DismissBanner(b.id)), "dismiss").p(px(2.0)).child(icon(
            Icon::X,
            14.0,
            p().ink_muted,
        )))
    });
    col().gap(px(6.0)).px(px(12.0)).pb(px(6.0)).children(rows).into_any_element()
}

/// In-app toasts, shown for 2 s under the title bar: a small floating sheet whose icon tells the level.
pub fn toasts(app: &App) -> El {
    if app.toasts.is_empty() {
        return nothing();
    }
    let items = app.toasts.iter().map(|toast| {
        let glyph = match toast.level {
            Level::Error => Icon::CircleAlert,
            Level::Warn => Icon::TriangleAlert,
            Level::Info => Icon::CircleCheckBig,
        };
        floating(row())
            .gap(px(tokens::SPACE_2))
            .px(px(14.0))
            .py(px(8.0))
            .child(icon(glyph, 14.0, p().ink))
            .child(line(toast.text.clone(), tokens::CAPTION, p().ink).medium())
    });
    col()
        .absolute()
        .top(px(44.0))
        .left(px(16.0))
        .right(px(16.0))
        .items_center()
        .gap(px(6.0))
        .children(items)
        .into_any_element()
}

/// While a file is held over the window (FR-18).
pub fn drop_overlay() -> El {
    let body = col()
        .items_center()
        .gap(px(tokens::SPACE_2))
        .child(icon(Icon::FileAudio, 28.0, p().ink_muted))
        .child(line(t("dropToTranscribe"), tokens::BODY_SMALL, p().ink_muted).medium());
    div()
        .absolute()
        .top(px(40.0))
        .left_0()
        .right_0()
        .bottom_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(p().surface)
        .child(body)
        .into_any_element()
}
