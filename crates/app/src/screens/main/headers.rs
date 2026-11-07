//! The tops of the recording and of a finished session. The recording keeps its project's colour
//! (FR-64), its title (click to rename) and what is recorded, a dropdown to switch it (FR-17), in
//! place above the transcript. A finished session's header scrolls with its transcript: the
//! title, when it started, its length, lines and screenshots in big numerals, and 詳細, closed at
//! first, for the model and the folder it was saved to.

use chidori_core::session::model::TimelineItem;
use gpui_kit::{InteractiveElement, IntoElement, ParentElement, Styled, div, px};

use super::source_picker;
use super::transcript::{ListMsg, ListState, Wrap};
use crate::app::{App, Message};
use crate::i18n::{is_ja, t, tf};
use crate::icons::{Icon, icon, turned};
use crate::model::format::{Unit, day_of, day_text, duration_parts, time_of};
use crate::model::transcript::Transcript;
use crate::screens::session_menu;
use crate::theme::{self, Color, Ink, line, p, project_color, tokens};
use crate::ui::{self, El, clickable};
use crate::widgets::{Size, Variant, btn_with, col, fill, hairline, row, swatch, with_tip};

/// A project's colour as a small rounded square; hollow for none.
fn mark(color: Option<Color>, size: f32) -> El {
    match color {
        Some(c) => swatch(c, size),
        None => div()
            .flex_none()
            .size(px(size))
            .border_1()
            .border_color(p().border_strong)
            .rounded(px(size * 0.3))
            .into_any_element(),
    }
}

/// The session's project, when it is one of the projects.
fn project_of(app: &App, tr: &Transcript) -> Option<(Color, String)> {
    let id = tr.project.as_deref()?;
    app.projects.iter().find(|p| p.id == id).map(|p| (project_color(&p.color), p.name.clone()))
}

fn project_menu(app: &App, tr: &Transcript) -> Option<Message> {
    let id = tr.session_id.clone()?;
    Some(Message::Popup(session_menu::project_items(app, &[id], tr.project.as_deref()), None))
}

/// A quiet button: the ground darkens under the pointer.
fn ghost(msg: Option<Message>, seed: &str) -> gpui_kit::Stateful<gpui_kit::Div> {
    let sunk = p().surface_sunken;
    clickable(msg, seed).rounded(px(tokens::RADIUS_MD)).text_color(p().ink).hover(move |s| s.bg(sunk))
}

/// A title as a button that renames it, or the box to rename it in.
fn title(name: String, renaming: bool, list: &ListState, style: tokens::TypeStyle, wrap: Wrap) -> El {
    if renaming {
        let input = ui::built_input(&list.title_key).unwrap_or_else(crate::widgets::nothing);
        let save = btn_with(
            icon(Icon::Check, 15.0, p().on_accent),
            Variant::Primary,
            Size::Md,
            Some(wrap(ListMsg::TitleSave)),
        )
        .w(px(36.0))
        .h(px(36.0));
        return row()
            .w_full()
            .gap(px(6.0))
            .child(div().flex_1().child(input))
            .child(with_tip(save, t("save")))
            .into_any_element();
    }
    let label = theme::t(name, style, p().ink).medium();
    let pencil = div()
        .absolute()
        .right(px(6.0))
        .top_0()
        .bottom_0()
        .flex()
        .items_center()
        .invisible()
        .group_hover("title", |s| s.visible())
        .child(icon(Icon::Pencil, 13.0, p().ink_muted));
    let base = ghost(Some(wrap(ListMsg::RenameStart)), "title")
        .group("title")
        .relative()
        .px(px(6.0))
        .py(px(2.0))
        .pr(px(24.0))
        .child(label)
        .child(pencil);
    with_tip(base, t("rename")).into_any_element()
}

/// The top of the recording view.
pub fn live_header(app: &App, tr: &Transcript, list: &ListState, wrap: Wrap) -> El {
    let project = project_of(app, tr);
    let label = project.as_ref().map_or_else(|| t("noProject"), |(_, n)| format!("{}: {n}", t("project")));
    let mut left = row().gap(px(4.0)).min_w_0();
    if tr.session_id.is_some() {
        let b = ghost(project_menu(app, tr), "project")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(28.0))
            .child(mark(project.map(|p| p.0), 11.0));
        left = left.child(with_tip(b, label));
    }
    left = left.child(div().flex_1().min_w_0().overflow_hidden().child(title(
        tr.title.clone().unwrap_or_default(),
        list.title.is_some(),
        list,
        tokens::LABEL,
        wrap,
    )));
    let mut l = div().min_w_0().child(left);
    l.style().flex_grow = Some(45.0);
    l.style().flex_basis = Some(px(0.0).into());
    let mut r = div().min_w_0().child(source_picker::dropdown(app));
    r.style().flex_grow = Some(55.0);
    r.style().flex_basis = Some(px(0.0).into());
    let top = row().w_full().gap(px(12.0)).py(px(8.0)).child(l).child(r);
    col().flex_none().w_full().px(px(18.0)).child(top).child(hairline()).into_any_element()
}

/// The last folders of a path, as Explorer's address bar reads: `Chidori › 2026-10-03_1513_Zoom`.
fn short_path(folder: &std::path::Path) -> String {
    let parts: Vec<String> = folder
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    parts[parts.len().saturating_sub(2)..].join(" › ")
}

/// Everyday and full name of a catalog model, and on what it ran.
fn model_text(app: &App, tr: &Transcript) -> String {
    let mut parts = Vec::new();
    if let Some(id) = &tr.model_id {
        let full = app
            .models
            .iter()
            .find(|m| &m.entry.id == id)
            .map_or_else(|| id.clone(), |m| if is_ja() { m.entry.name_ja.clone() } else { m.entry.name_en.clone() });
        parts.push(full);
    }
    if let Some(gpu) = tr.gpu {
        parts.push(t(if gpu { "gpu" } else { "cpu" }));
    }
    parts.join(" · ")
}

/// A finished session's header, built whenever its row is (it scrolls with the transcript).
pub fn session_header(app: &App, tr: &Transcript, list: &ListState, wrap: Wrap) -> El {
    let name = tr.title.clone().unwrap_or_default();
    let details = list.details;
    let project = project_of(app, tr);
    let menu = project_menu(app, tr);
    let when = tr.started_at.as_ref().map(|s| {
        let day = day_text(day_of(s), chrono::Datelike::year(&chrono::Local::now()));
        let when = if is_ja() { format!("{day}{}", time_of(s)) } else { format!("{day}, {}", time_of(s)) };
        tf("startedWhen", &[("when", &when)])
    });
    let duration = tr.duration_ms;
    let lines = tr.items.iter().filter(|i| matches!(i, TimelineItem::Segment(_))).count();
    let shots = tr.items.iter().filter(|i| matches!(i, TimelineItem::Screenshot(_))).count();
    let model = model_text(app, tr);
    let folder = tr.folder.clone();
    let session = tr.session_id.clone();

    let mut c = col().w_full().child(title(name, list.title.is_some(), list, tokens::HEADING, wrap));
    let mut meta = row().gap(px(10.0)).pt(px(2.0));
    if session.is_some() {
        let label = project.as_ref().map_or_else(|| t("noProject"), |(_, n)| n.clone());
        let tone = if project.is_some() { p().ink } else { p().ink_muted };
        let text = line(label, tokens::CAPTION, tone);
        let text = if project.is_some() { text.medium() } else { text };
        let r = row().gap(px(6.0)).child(mark(project.as_ref().map(|p| p.0), 10.0)).child(text).child(icon(
            Icon::ChevronDown,
            12.0,
            p().ink_muted,
        ));
        meta = meta.child(ghost(menu, "project-meta").px(px(4.0)).py(px(2.0)).child(r));
    }
    if let Some(w) = &when {
        meta = meta.child(line(w.clone(), tokens::CAPTION, p().ink_muted));
    }
    c = c.child(meta);

    let big = |n: u64| line(n.to_string(), tokens::TITLE, p().ink).mono();
    let unit = |s: String| line(s, tokens::CAPTION, p().ink_muted).pb(px(4.0));
    let mut numbers = div().flex().items_end().gap(px(18.0)).pt(px(10.0));
    if let Some(ms) = duration {
        let mut d = div().flex().items_end().gap(px(2.0));
        for (n, u) in duration_parts(ms) {
            let u = t(match u {
                Unit::H => "unitH",
                Unit::M => "unitM",
                Unit::S => "unitS",
            });
            d = d.child(big(n)).child(unit(u).pr(px(4.0)));
        }
        numbers = numbers.child(d);
    }
    numbers = numbers.child(div().flex().items_end().gap(px(2.0)).child(big(lines as u64)).child(unit(t("unitLines"))));
    if shots > 0 {
        numbers =
            numbers.child(div().flex().items_end().gap(px(2.0)).child(big(shots as u64)).child(unit(t("unitShots"))));
    }
    numbers = numbers.child(fill());
    if !model.is_empty() || folder.is_some() {
        let chevron = turned(Icon::ChevronDown, 13.0, p().ink_muted, if details { 0.5 } else { 0.0 });
        let toggle = ghost(Some(wrap(ListMsg::Details)), "details")
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .mb(px(4.0))
            .child(line(t("details"), tokens::CAPTION, p().ink_muted).medium())
            .child(chevron);
        numbers = numbers.child(toggle);
    }
    c = c.child(numbers);

    if details {
        let mut dl = col().gap(px(4.0)).pt(px(10.0));
        let entry = |k: String, v: El| -> El {
            row()
                .gap(px(12.0))
                .child(div().flex_none().w(px(64.0)).child(line(k, tokens::CAPTION, p().ink_muted)))
                .child(div().flex_1().min_w_0().overflow_hidden().child(v))
                .into_any_element()
        };
        if !model.is_empty() {
            dl = dl.child(entry(t("model"), line(model.clone(), tokens::CAPTION, p().ink).into_any_element()));
        }
        if let Some(f) = &folder {
            let link = clickable(Some(Message::Action(crate::actions::Action::OpenFolder(session.clone()))), "folder")
                .child(line(short_path(f), tokens::CAPTION, p().ink).underline());
            dl = dl.child(entry(t("savedTo"), with_tip(link, f.to_string_lossy().into_owned()).into_any_element()));
        }
        c = c.child(dl);
    }
    col().w_full().px(px(18.0)).child(div().pt(px(12.0)).pb(px(14.0)).child(c)).child(hairline()).into_any_element()
}
