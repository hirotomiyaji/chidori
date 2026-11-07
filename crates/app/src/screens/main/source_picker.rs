//! What to listen to (FR-10 to FR-13): an app, everything the PC plays, or the mic alone, and
//! whether it is a conversation, when the mic records your voice too and the transcript names the
//! lines 相手 and 自分. Before a recording it is the start screen's two fields (the compact window
//! shows the choice as one line opening the picker over the window); while recording it is the
//! header's dropdown, where a pick switches the source at once (FR-17).

use chidori_core::audio::source::AudioApp;
use chidori_core::settings::SourceMode;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{InteractiveElement, IntoElement, ParentElement, Styled, div, img, px};

use super::popover::popover;
use super::{Msg, app_wave};
use crate::app::{App, Message, call};
use crate::i18n::t;
use crate::icons::turned;
use crate::icons::{Icon, icon};
use crate::task::Task;
use crate::theme::{Ink, alpha, line, p, tokens};
use crate::ui::{El, clickable, uid};
use crate::widgets::{Size, col, fill, floating, icon_button, row, spinner, with_tip};

/// Which list is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Open {
    #[default]
    None,
    Source,
    Mic,
    /// The recording header's dropdown.
    Switch,
}

#[derive(Debug, Default)]
pub struct PickerState {
    pub open: Open,
    /// The compact window's picker over the whole window.
    pub panel: bool,
}

#[derive(Debug, Clone)]
pub enum PickMsg {
    Toggle(Open),
    Close,
    Pick(SourceMode, Option<u32>),
    Refresh,
    Refreshed,
    Conversation,
    Mic(Option<String>),
    Panel(bool),
}

fn m(msg: PickMsg) -> Message {
    Message::Main(Msg::Pick(msg))
}

/// The apps whose sound is on screen: the chosen app's, and while a list is open, every app with
/// an audio session in it.
pub fn shown_pids(app: &App) -> Vec<u32> {
    let s = &app.source;
    let mut pids = Vec::new();
    if s.mode == SourceMode::App
        && let Some(a) = &s.app
        && a.root_pid != 0
    {
        pids.push(a.root_pid);
    }
    if matches!(app.main.picker.open, Open::Source | Open::Switch) {
        pids.extend(s.apps.iter().filter(|a| a.has_session).map(|a| a.root_pid));
    }
    pids
}

pub fn update(app: &mut App, msg: PickMsg) -> Task {
    match msg {
        PickMsg::Toggle(which) => {
            let st = &mut app.main.picker;
            if st.open == which {
                st.open = Open::None;
                return Task::none();
            }
            st.open = which;
            match which {
                Open::Mic => app.refresh_devices(),
                Open::None => Task::none(),
                _ => refresh(app),
            }
        }
        PickMsg::Close => {
            app.main.picker.open = Open::None;
            Task::none()
        }
        PickMsg::Pick(mode, pid) => {
            if let Some(pid) = pid
                && let Some(a) = app.source.apps.iter().find(|a| a.root_pid == pid).cloned()
            {
                app.source.set_app(&a);
            }
            app.source.mode = mode;
            app.main.picker.open = Open::None;
            super::source_changed(app)
        }
        PickMsg::Refresh => refresh(app),
        PickMsg::Refreshed => Task::none(),
        PickMsg::Conversation => {
            app.source.include_mic = !app.source.include_mic;
            super::source_changed(app)
        }
        PickMsg::Mic(id) => {
            app.source.mic_device_id = id.clone();
            app.main.picker.open = Open::None;
            Task::batch([
                Task::done(Message::UpdateSettings(serde_json::json!({ "source": { "micDeviceId": id } }))),
                super::source_changed(app),
            ])
        }
        PickMsg::Panel(open) => {
            app.main.picker.panel = open;
            app.main.picker.open = Open::None;
            if open { refresh(app) } else { Task::none() }
        }
    }
}

/// A fresh app list; the waves follow it.
pub fn refresh(app: &mut App) -> Task {
    app.refresh_apps().chain(Task::done(m(PickMsg::Refreshed)))
}

/// Tells the core which apps' sound to read when that changed.
pub fn sync_watch(app: &mut App, pids: Vec<u32>) -> Task {
    match app.app_levels.watch(pids, app.now) {
        Some(pids) => call(app, move |c| chidori_core::ops::watch_app_levels(c, pids), |_| Message::Noop),
        None => Task::none(),
    }
}

/// An app's icon, or the window glyph when it has none.
fn app_icon(app: &App, exe: Option<&str>, size: f32) -> El {
    match exe.and_then(|e| app.source.icon_of(e)) {
        Some(h) => img(h.0.clone()).flex_none().size(px(size)).into_any_element(),
        None => icon(Icon::AppWindow, size - 2.0, p().ink_muted).into_any_element(),
    }
}

/// A field: a box on the raised ground, ink-edged while its list is open.
fn field_box(d: gpui_kit::Stateful<gpui_kit::Div>, open: bool) -> gpui_kit::Stateful<gpui_kit::Div> {
    let p = p();
    let strong = p.border_strong;
    let d = d.bg(p.surface_raised).text_color(p.ink).border_1().rounded(px(tokens::RADIUS_MD));
    if open { d.border_color(p.ink) } else { d.border_color(p.border).hover(move |s| s.border_color(strong)) }
}

/// How a switch looks, inside a control that is a switch as a whole: a squared track.
fn switch_mark(on: bool) -> El {
    let knob = div().size(px(16.0)).rounded(px(4.0)).bg(p().surface_raised);
    let track = div().flex().flex_none().w(px(36.0)).h(px(20.0)).p(px(2.0)).rounded(px(tokens::RADIUS_SM)).bg(if on {
        p().accent
    } else {
        p().border_strong
    });
    if on { track.justify_end() } else { track.justify_start() }.child(knob).into_any_element()
}

/// One choice in a list: its icon, name, anything extra (the app's wave) and a check, kept in
/// every row so the waves line up.
fn option(selected: bool, enabled: bool, glyph: El, label: String, extra: Option<El>, on: Message) -> El {
    let name = line(label, tokens::BODY_SMALL, p().ink);
    let name = if selected { name.medium() } else { name };
    let mut r = row()
        .w_full()
        .gap(px(10.0))
        .child(div().flex().flex_none().items_center().justify_center().size(px(20.0)).child(glyph))
        .child(div().flex_1().min_w_0().overflow_hidden().child(name));
    if let Some(e) = extra {
        r = r.child(e);
    }
    let check: El = if selected {
        icon(Icon::Check, 14.0, p().ink).into_any_element()
    } else {
        div().flex_none().size(px(14.0)).into_any_element()
    };
    r = r.child(check);
    let sunk = p().surface_sunken;
    let b = clickable(enabled.then_some(on), "option").w_full().px(px(10.0)).py(px(8.0)).rounded(px(tokens::RADIUS_MD));
    let b = if enabled { b.hover(move |s| s.bg(sunk)) } else { b.opacity(0.45) };
    b.child(r).into_any_element()
}

fn group(label: String, action: Option<El>) -> El {
    let mut r = row().child(div().flex_1().child(line(label, tokens::CAPTION, p().ink_muted).medium()));
    if let Some(a) = action {
        r = r.child(a);
    }
    div().px(px(10.0)).pt(px(8.0)).pb(px(2.0)).child(r).into_any_element()
}

/// The sources to pick from: システム全体, マイクだけ, then the apps, those playing first (FR-11).
fn options(app: &App) -> Vec<El> {
    let s = &app.source;
    let supported = app.info.app_loopback_supported;
    let mut list = vec![
        option(
            s.mode == SourceMode::System,
            true,
            icon(Icon::MonitorSpeaker, 18.0, p().ink).into_any_element(),
            t("srcSystem"),
            None,
            m(PickMsg::Pick(SourceMode::System, None)),
        ),
        option(
            s.mode == SourceMode::Mic,
            true,
            icon(Icon::Mic, 18.0, p().ink).into_any_element(),
            t("srcMicOnly"),
            None,
            m(PickMsg::Pick(SourceMode::Mic, None)),
        ),
    ];
    let refresh: El = if s.loading_apps {
        div().flex().items_center().justify_center().size(px(24.0)).child(spinner(13.0, app.now)).into_any_element()
    } else {
        icon_button(Icon::RefreshCw, t("refresh"), Size::Sm, Some(m(PickMsg::Refresh)))
    };
    list.push(group(t("srcApps"), Some(refresh)));
    let app_option = |a: &AudioApp| {
        let o = option(
            s.is_chosen(a),
            supported,
            app_icon(app, Some(&a.exe), 20.0),
            a.name.clone(),
            a.has_session.then(|| app_wave::wave(app, Some(a.root_pid), 30.0, 14.0)),
            m(PickMsg::Pick(SourceMode::App, Some(a.root_pid))),
        );
        if supported { o } else { with_tip(o, t("win11Required")).into_any_element() }
    };
    list.extend(s.apps.iter().filter(|a| a.has_session).map(app_option));
    if s.apps.iter().any(|a| !a.has_session) {
        list.push(group(t("otherApps"), None));
    }
    list.extend(s.apps.iter().filter(|a| !a.has_session).map(app_option));
    if s.apps.is_empty() {
        let note = if s.loading_apps { t("loading") } else { t("noApps") };
        list.push(
            div()
                .w_full()
                .flex()
                .justify_center()
                .px(px(10.0))
                .py(px(12.0))
                .child(line(note, tokens::CAPTION, p().ink_muted))
                .into_any_element(),
        );
    }
    list
}

/// The floating sheet a list sits on.
fn sheet(items: Vec<El>) -> El {
    floating(col()).w_full().p(px(4.0)).children(items).into_any_element()
}

/// The chosen source's icon and name.
fn chosen(app: &App, dense: bool) -> (El, String, bool) {
    let s = &app.source;
    let size = if dense { 17.0 } else { 20.0 };
    match s.mode {
        SourceMode::App => (
            app_icon(app, s.app.as_ref().map(|a| a.exe.as_str()), if dense { 18.0 } else { 22.0 }),
            s.app.as_ref().map_or_else(|| t("chooseApp"), |a| a.name.clone()),
            s.app.is_none(),
        ),
        SourceMode::System => (icon(Icon::MonitorSpeaker, size, p().ink).into_any_element(), t("srcSystem"), false),
        SourceMode::Mic => (icon(Icon::Mic, size, p().ink).into_any_element(), t("srcMicOnly"), false),
    }
}

fn chevron(open: bool) -> El {
    turned(Icon::ChevronDown, 15.0, p().ink_muted, if open { 0.5 } else { 0.0 }).into_any_element()
}

fn boxed(glyph: El, side: f32) -> El {
    div().flex().flex_none().items_center().justify_center().size(px(side)).child(glyph).into_any_element()
}

/// What 相手 is: an app, everything the PC plays, or nothing but the mic (FR-10, FR-11).
fn source_field(app: &App, dense: bool) -> El {
    let open = app.main.picker.open == Open::Source;
    let (glyph, name, missing) = chosen(app, dense);
    let mut r = row().w_full().h_full().gap(px(10.0)).child(boxed(glyph, 22.0)).child(
        div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .child(line(name, tokens::BODY_SMALL, if missing { p().ink_muted } else { p().ink }).medium()),
    );
    if app.source.mode == SourceMode::App {
        let pid = app.source.app.as_ref().map(|a| a.root_pid);
        r = r.child(if dense { app_wave::wave(app, pid, 30.0, 14.0) } else { app_wave::wave(app, pid, 36.0, 16.0) });
    }
    r = r.child(chevron(open));
    let anchor = field_box(clickable(Some(m(PickMsg::Toggle(Open::Source))), "source-field"), open)
        .w_full()
        .h(px(if dense { 40.0 } else { 48.0 }))
        .px(px(12.0))
        .child(r);
    popover(anchor, open.then(|| sheet(options(app))), m(PickMsg::Close)).min_width(280.0).el()
}

/// Whether to record a conversation (「会話として録音」, FR-12), or with the mic as the only
/// source, just which mic it is (FR-13).
fn mic_field(app: &App, dense: bool, only: bool) -> El {
    let s = &app.source;
    let open = app.main.picker.open == Open::Mic;
    let on = only || s.include_mic;
    let tone = if on { p().ink } else { p().ink_muted };
    let device = s.mic_name().filter(|_| s.mic_device_id.is_some()).unwrap_or_else(|| t("defaultDevice"));
    let mut labels = col()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .child(line(if only { t("srcMic") } else { t("conversation") }, tokens::BODY_SMALL, tone).medium());
    if !dense {
        labels = labels.child(line(device, tokens::CAPTION, p().ink_muted));
    }
    let body = row()
        .flex_1()
        .min_w_0()
        .gap(px(10.0))
        .child(icon(Icon::Mic, if dense { 17.0 } else { 19.0 }, tone))
        .child(labels);
    let main: El = if only {
        body.into_any_element()
    } else {
        clickable(Some(m(PickMsg::Conversation)), "conversation")
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(10.0))
            .child(body)
            .child(switch_mark(s.include_mic))
            .into_any_element()
    };
    let anchor = row()
        .w_full()
        .gap(px(4.0))
        .h(px(if dense { 40.0 } else { 48.0 }))
        .pl(px(12.0))
        .pr(px(6.0))
        .bg(p().surface_raised)
        .border_1()
        .border_color(if open { p().ink } else { p().border })
        .rounded(px(tokens::RADIUS_MD))
        .child(main)
        .child(icon_button(Icon::ChevronDown, t("chooseMic"), Size::Sm, Some(m(PickMsg::Toggle(Open::Mic)))));
    let list = open.then(|| {
        let mut items = vec![option(
            s.mic_device_id.is_none(),
            true,
            icon(Icon::Mic, 16.0, p().ink).into_any_element(),
            t("defaultDevice"),
            None,
            m(PickMsg::Mic(None)),
        )];
        items.extend(s.devices.iter().map(|d| {
            option(
                s.mic_device_id.as_deref() == Some(d.id.as_str()),
                true,
                icon(Icon::Mic, 16.0, p().ink).into_any_element(),
                d.name.clone(),
                None,
                m(PickMsg::Mic(Some(d.id.clone()))),
            )
        }));
        sheet(items)
    });
    popover(anchor, list, m(PickMsg::Close)).min_width(280.0).el()
}

/// The source and the mic, as two fields.
pub fn picker(app: &App, dense: bool) -> El {
    col()
        .w_full()
        .gap(px(if dense { 6.0 } else { 8.0 }))
        .child(source_field(app, dense))
        .child(mic_field(app, dense, app.source.mode == SourceMode::Mic))
        .into_any_element()
}

/// The choice in one line, `Zoom · 会話`, opening the picker.
pub fn summary(app: &App, open: bool, height: f32, on: Message) -> El {
    let s = &app.source;
    let (glyph, name, _) = chosen(app, true);
    let mut r = row()
        .w_full()
        .h_full()
        .gap(px(8.0))
        .child(boxed(glyph, 18.0))
        .child(div().min_w_0().overflow_hidden().child(line(name, tokens::BODY_SMALL, p().ink).medium()));
    if s.mode != SourceMode::Mic && s.include_mic {
        r = r.child(line(format!("· {}", t("conversationShort")), tokens::CAPTION, p().ink_muted).flex_none());
    }
    r = r.child(fill());
    if s.mode == SourceMode::App {
        r = r.child(app_wave::wave(app, s.app.as_ref().map(|a| a.root_pid), 30.0, 14.0));
    }
    r = r.child(chevron(open));
    let b = field_box(clickable(Some(on), "summary"), open).w_full().h(px(height)).px(px(12.0)).child(r);
    with_tip(b, t("chooseSource")).w_full().into_any_element()
}

/// The recording's source as a dropdown under its one line (FR-17): the sources, and
/// 会話として録音 under them. A pick switches the source at once.
pub fn dropdown(app: &App) -> El {
    let open = app.main.picker.open == Open::Switch;
    let anchor = summary(app, open, 36.0, m(PickMsg::Toggle(Open::Switch)));
    let list = open.then(|| {
        let mut items = options(app);
        if app.source.mode != SourceMode::Mic {
            let on = app.source.include_mic;
            let sunk = p().surface_sunken;
            let r = row()
                .w_full()
                .gap(px(10.0))
                .child(icon(Icon::Mic, 18.0, if on { p().ink } else { p().ink_muted }))
                .child(div().flex_1().min_w_0().child(line(t("conversation"), tokens::BODY_SMALL, p().ink)))
                .child(switch_mark(on));
            items.push(crate::widgets::hairline());
            items.push(
                clickable(Some(m(PickMsg::Conversation)), "dropdown-conversation")
                    .w_full()
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(tokens::RADIUS_MD))
                    .hover(move |s| s.bg(sunk))
                    .child(r)
                    .into_any_element(),
            );
        }
        sheet(items)
    });
    popover(anchor, list, m(PickMsg::Close)).min_width(280.0).align_end().el()
}

/// The full picker over the window under the title bar: the compact window's way to choose before
/// a recording, and to transcribe a file instead (FR-18).
pub fn panel(app: &App) -> El {
    let head = row().child(div().flex_1().child(line(t("listenTo"), tokens::CAPTION, p().ink_muted).medium())).child(
        crate::widgets::icon_button_tinted(
            Icon::Check,
            t("done"),
            Size::Sm,
            Some(m(PickMsg::Panel(false))),
            Some(p().ink),
        ),
    );
    let body = col()
        .gap(px(4.0))
        .px(px(12.0))
        .pt(px(2.0))
        .pb(px(8.0))
        .child(head)
        .child(picker(app, true))
        .child(file_link(app, true));
    div()
        .id(uid("panel"))
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .bg(p().surface)
        .overflow_y_scrollbar()
        .occlude()
        .child(body)
        .into_any_element()
}

/// 「ファイルから文字起こし」: a recording made elsewhere, transcribed as fast as the PC can.
pub fn file_link(app: &App, close_panel: bool) -> El {
    let enabled = app.rec.state == chidori_core::events::UiState::Ready;
    let ink = p().ink;
    let content = row()
        .gap(px(6.0))
        .child(
            icon(Icon::FileAudio, 13.0, if enabled { p().ink_muted } else { alpha(p().ink_muted, 0.45) })
                .group_hover("file-link", move |s| s.text_color(ink)),
        )
        .child(div().text_size(px(tokens::CAPTION.size)).medium().child(t("transcribeFile")));
    if !enabled {
        return div().px(px(4.0)).py(px(2.0)).text_color(alpha(p().ink_muted, 0.45)).child(content).into_any_element();
    }
    let action = Message::Action(crate::actions::Action::TranscribeFile);
    let msg = if close_panel { Message::Main(Msg::Batch(vec![m(PickMsg::Panel(false)), action])) } else { action };
    clickable(Some(msg), "file-link")
        .group("file-link")
        .px(px(4.0))
        .py(px(2.0))
        .text_color(p().ink_muted)
        .hover(move |s| s.text_color(ink))
        .child(content)
        .into_any_element()
}
