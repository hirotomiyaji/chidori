//! The title bar over every screen, kept while the views under it change (DECISIONS → UI, "One
//! title bar"): back (a past session, a section of 設定), the name or what is recording, the three
//! places as words (expanded) or icons (compact), and the window buttons. Dragging it moves the
//! window. No rule under it: only the setup wizard draws one, under its title.

use chidori_core::events::UiState;
use chidori_core::settings::{Layout, SourceMode};
use gpui_kit::{InteractiveElement, IntoElement, ParentElement, Styled, WindowControlArea, div, px};

use crate::app::{App, Message, Place, SettingsTab, View, WinAction};
use crate::i18n::t;
use crate::icons::{Icon, icon};
use crate::model::format::sources_text;
use crate::native::menu::Item;
use crate::task::Task;
use crate::theme::{Ink, line, p, tokens};
use crate::ui::{El, clickable};
use crate::widgets::{Size, bare, divider, icon_button, icon_button_tinted, row, with_tip};

#[derive(Debug, Clone)]
pub enum Msg {
    Back,
    /// The compact window's 録音中 tag: a native menu to switch the source (FR-17).
    SourceMenu,
    /// The app list is fresh: the menu opens.
    SourceMenuReady,
}

/// Where the back arrow leads: a past session back to 履歴, a section of 設定 back to its list.
fn back_of(app: &App) -> Option<Msg> {
    if app.needs_wizard() {
        return None;
    }
    match app.view {
        View::Session => Some(Msg::Back),
        View::Settings if app.settings_tab != SettingsTab::Index => Some(Msg::Back),
        _ => None,
    }
}

pub fn update(app: &mut App, msg: Msg) -> Task {
    match msg {
        Msg::Back => match app.view {
            View::Session => Task::done(Message::Go(View::History)),
            View::Settings => {
                app.settings_tab = SettingsTab::Index;
                Task::none()
            }
            _ => Task::none(),
        },
        Msg::SourceMenu => {
            let refresh = app.refresh_apps();
            refresh.chain(Task::done(Message::Titlebar(Msg::SourceMenuReady)))
        }
        Msg::SourceMenuReady => Task::done(Message::Popup(source_menu(app), None)),
    }
}

/// The source menu, once the app list is fresh.
fn source_menu(app: &App) -> Vec<Item> {
    let s = &app.source;
    let supported = app.info.app_loopback_supported;
    let mut items = vec![
        Item::check("titlebar:src-system", t("srcSystem"), s.mode == SourceMode::System),
        Item::check("titlebar:src-mic", t("srcMicOnly"), s.mode == SourceMode::Mic),
        Item::Separator,
    ];
    let playing = s.apps.iter().filter(|a| a.has_session);
    let others = s.apps.iter().filter(|a| !a.has_session);
    for a in playing.chain(others) {
        items.push(
            Item::check(format!("titlebar:src-app:{}", a.root_pid), a.name.clone(), s.is_chosen(a)).enabled(supported),
        );
    }
    if !s.apps.is_empty() {
        items.push(Item::Separator);
    }
    items.push(
        Item::check("titlebar:src-conversation", t("conversation"), s.mode != SourceMode::Mic && s.include_mic)
            .enabled(s.mode != SourceMode::Mic),
    );
    items
}

/// A pick in the source menu changes the choice; the main view turns it into a switch.
pub fn on_menu(app: &mut App, rest: &str) -> Task {
    let s = &mut app.source;
    match rest.split_once(':') {
        Some(("src-app", pid)) => {
            if let Some(a) = s.apps.iter().find(|a| a.root_pid.to_string() == pid).cloned() {
                s.set_app(&a);
                s.mode = SourceMode::App;
            }
        }
        _ => match rest {
            "src-system" => s.mode = SourceMode::System,
            "src-mic" => s.mode = SourceMode::Mic,
            "src-conversation" => s.include_mic = !s.include_mic,
            _ => return Task::none(),
        },
    }
    super::main_view::source_changed(app)
}

/// What is recording, in place of the name while a recording runs: a mark in the accent (grey
/// while paused or finishing), the state and the source.
fn live_tag(app: &App) -> El {
    let state = app.rec.state;
    let quiet = matches!(state, UiState::Paused | UiState::Finishing);
    let from_file =
        app.rec.sources.as_ref().is_some_and(|s| s.ids.contains(&chidori_core::session::model::SourceId::File));
    let label = match state {
        UiState::Paused => t("paused"),
        UiState::Finishing => t(if from_file { "transcribingFile" } else { "finishing" }),
        _ => t("recording"),
    };
    let sources = sources_text(app.rec.sources.as_ref());
    let mark = div().flex_none().size(px(8.0)).rounded(px(2.0)).bg(if quiet { p().border_strong } else { p().accent });
    let mut body = row()
        .gap(px(tokens::SPACE_2))
        .px(px(4.0))
        .py(px(2.0))
        .overflow_hidden()
        .child(mark)
        .child(line(label, tokens::CAPTION, p().ink).medium().flex_none());
    if !sources.is_empty() {
        body = body.child(line(sources.clone(), tokens::CAPTION, p().ink_muted).min_w_0());
    }
    if app.view == View::Main && app.compact() && state != UiState::Finishing {
        return with_tip(bare(body, Some(Message::Titlebar(Msg::SourceMenu)), true), t("switchSource"))
            .min_w_0()
            .into_any_element();
    }
    if app.view == View::Main {
        // The title bar may be too narrow for the source: it shows on hover.
        return with_tip(body, sources).min_w_0().into_any_element();
    }
    with_tip(bare(body, Some(Message::Go(View::Main)), true), t("backToLive")).min_w_0().into_any_element()
}

/// The name, with the app's kamon as a window's icon sits beside its title.
fn brand() -> El {
    row()
        .gap(px(tokens::SPACE_2))
        .child(crate::icons::logo(18.0))
        .child(line("Chidori", tokens::BODY_SMALL, p().ink).medium())
        .into_any_element()
}

/// 録音 · 履歴 · 設定 as words; the current place is in the accent's tint, and 録音 carries a mark
/// while recording.
fn places(app: &App) -> El {
    let busy = app.rec.busy();
    let current = app.view.place();
    let entries = [
        (Place::Main, t("tabRecord"), View::Main),
        (Place::History, t("history"), View::History),
        (Place::Settings, t("settings"), View::Settings),
    ];
    let mut r = row().flex_none();
    for (place, label, view) in entries {
        let here = place == current;
        let mut content = row().gap(px(6.0)).px(px(7.0)).py(px(3.0));
        if place == Place::Main && busy {
            content = content.child(div().flex_none().size(px(6.0)).rounded(px(2.0)).bg(p().accent));
        }
        let color = if here { p().accent_deep } else { p().ink_muted };
        content = content.child(line(label, tokens::BODY_SMALL, color).medium());
        let mut b = clickable(Some(Message::Go(view)), "place").rounded(px(tokens::RADIUS_SM)).child(content);
        if here {
            b = b.bg(p().accent_tint);
        } else {
            let sunk = p().surface_sunken;
            b = b.hover(move |s| s.bg(sunk));
        }
        let tip = if place == Place::Main && busy && !here { t("backToLive") } else { String::new() };
        r = r.child(with_tip(b, tip));
    }
    r.into_any_element()
}

fn window_buttons(app: &App, layout_toggle: bool) -> El {
    let on_top = app.settings.window.always_on_top;
    let compact = app.view == View::Main && app.layout() == Layout::Compact;
    let mut r = row().flex_none().child(icon_button_tinted(
        Icon::Pin,
        t("alwaysOnTop"),
        Size::Sm,
        Some(Message::Win(WinAction::ToggleOnTop)),
        on_top.then(|| p().ink),
    ));
    if layout_toggle {
        let (glyph, label) = if compact { (Icon::Maximize, t("expand")) } else { (Icon::Minimize, t("shrink")) };
        r = r.child(icon_button(glyph, label, Size::Sm, Some(Message::Win(WinAction::ToggleLayout))));
    }
    r.child(icon_button(Icon::Minus, t("minimize"), Size::Sm, Some(Message::Win(WinAction::Minimize))))
        .child(close_button())
        .into_any_element()
}

/// Close turns brick on hover, like the system's own.
fn close_button() -> El {
    let surface = p().surface;
    let danger = p().danger;
    let b = clickable(Some(Message::Win(WinAction::Close)), "close")
        .group("close")
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.0))
        .rounded(px(tokens::RADIUS_MD))
        .hover(move |s| s.bg(danger))
        .child(icon(Icon::X, 15.0, p().ink_muted).group_hover("close", move |s| s.text_color(surface)));
    with_tip(b, t("close")).into_any_element()
}

pub fn view(app: &App) -> El {
    let setup = app.needs_wizard();
    let compact = app.compact();
    let mut bar = row().flex_none().gap(px(4.0)).h(px(40.0)).pl(px(12.0)).pr(px(6.0));
    if back_of(app).is_some() {
        bar = bar.child(solid(icon_button(Icon::ArrowLeft, t("back"), Size::Sm, Some(Message::Titlebar(Msg::Back)))));
    }
    let lead: El = if app.rec.busy() { solid(live_tag(app)) } else { brand() };
    bar = bar.child(row().flex_1().min_w_0().overflow_hidden().child(lead));
    if !setup {
        if compact {
            bar = bar
                .child(solid(icon_button(Icon::History, t("history"), Size::Sm, Some(Message::Go(View::History)))))
                .child(solid(icon_button(Icon::Settings, t("settings"), Size::Sm, Some(Message::Go(View::Settings)))));
        } else {
            bar = bar.child(solid(places(app)));
        }
        bar = bar.child(div().px(px(4.0)).child(divider()));
    }
    bar = bar.child(solid(window_buttons(app, !setup)));
    // Where nothing else takes the pointer, the bar moves the window (Windows' own caption drag).
    bar.id("titlebar").window_control_area(WindowControlArea::Drag).into_any_element()
}

/// Keeps the bar's drag area from under what can be clicked: Windows asks GPUI what the pointer
/// is over, and any drag area still under it makes the click a window drag.
fn solid(content: El) -> El {
    div().id(crate::ui::uid("solid")).flex().flex_none().occlude().child(content).into_any_element()
}
