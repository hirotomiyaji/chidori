//! 履歴: past sessions by day, each with its own line; searchable by title and opening words
//! (FR-62, FR-63), and narrowed to one project (FR-64), which makes the page that project's: its
//! name on top and its own menu to change or delete it. 選択 picks several to move to the Recycle
//! Bin at once, such as all the short and empty ones, or to put in a project.

use std::collections::HashSet;

use chidori_core::ops;
use chidori_core::session::model::SourceId;
use chidori_core::session::projects::Project;
use chidori_core::session::recovery::SessionSummary;
use chrono::{Datelike, Local, NaiveDate};
use gpui_kit::{InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, px};

use super::dialogs::{self, project_mark};
use super::session_menu::{self, SessionRef, menu_text};
use super::{form, session_line};
use crate::app::{App, Message, View, call};
use crate::i18n::{t, tf};
use crate::icons::{Icon, icon};
use crate::model::format::{DayName, day_name, day_of, day_text, duration_text, time_of};
use crate::model::line::{Shape, shape_from_activity};
use crate::native::menu::Item;
use crate::task::{self, Task};
use crate::theme::{self, Ink, line, p, plain, tokens};
use crate::ui::{El, Field, KeyPress, Ui, clickable};
use crate::widgets::{Size, Variant, btn, col, hairline, icon_button, row, with_tip};

const SEARCH: &str = "history-search";

/// Shorter than this, a recording is most likely a test or started by accident.
const SHORT_MS: u64 = 10_000;

#[derive(Debug, Default)]
pub struct State {
    /// None until the list first loads.
    sessions: Option<Vec<Entry>>,
    query: String,
    selecting: bool,
    selected: HashSet<String>,
    filter: Filter,
    /// Today, as of when the list loaded, for 今日 and 昨日.
    today: Option<NaiveDate>,
}

/// A row: the session and its line, built once when the list loads.
#[derive(Debug, Clone)]
struct Entry {
    s: SessionSummary,
    shape: Shape,
    /// It has both sides, so its line has 自分 below the baseline.
    two: bool,
}

impl Entry {
    fn new(s: SessionSummary) -> Self {
        let two = s.sources.contains(&SourceId::Mic) && s.sources.iter().any(|id| *id != SourceId::Mic);
        Entry { shape: shape_from_activity(&s.activity), two, s }
    }
}

/// Which sessions the list shows: all, those in no project, or one project's.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    None,
    Project(String),
}

#[derive(Debug, Clone)]
pub enum Msg {
    Loaded(Vec<SessionSummary>),
    Query(String),
    Escape,
    ToggleSelecting,
    Open(String),
    RowMenu(String),
    FilterMenu,
    /// The project's own menu, on its page.
    PageMenu,
    /// Selects every row shown, or none once all are.
    ChooseAll,
    ChooseEmpty,
    ChosenProjectMenu,
    DeleteChosen,
}

fn msg(m: Msg) -> Message {
    Message::History(m)
}

/// A recording under 10 s or with no transcript: greyed out (still readable, still openable),
/// with no line.
fn is_empty_session(s: &SessionSummary) -> bool {
    s.segments == 0 || s.duration_ms < SHORT_MS
}

/// Text as search compares it: NFKC (full-width letters and digits as ASCII, half-width kana as
/// full-width), any case.
fn norm(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization as _;
    s.nfkc().flat_map(char::to_lowercase).collect()
}

fn known<'a>(projects: &'a [Project], id: Option<&str>) -> Option<&'a Project> {
    id.and_then(|id| projects.iter().find(|p| p.id == id))
}

/// A deleted project can't stay the filter.
fn effective(filter: &Filter, projects: &[Project]) -> Filter {
    match filter {
        Filter::Project(id) if known(projects, Some(id)).is_none() => Filter::All,
        f => f.clone(),
    }
}

/// The sessions the search and the filter let through, by day, in the list's order.
fn days<'a>(
    entries: &'a [Entry],
    query: &str,
    filter: &Filter,
    projects: &[Project],
) -> Vec<(&'a str, Vec<&'a Entry>)> {
    let q = norm(query.trim());
    let mut out: Vec<(&str, Vec<&Entry>)> = Vec::new();
    for e in entries {
        let project = known(projects, e.s.project.as_deref());
        let in_filter = match filter {
            Filter::All => true,
            Filter::None => project.is_none(),
            Filter::Project(id) => project.is_some_and(|p| &p.id == id),
        };
        if !in_filter || !(q.is_empty() || norm(&e.s.title).contains(&q) || norm(&e.s.preview).contains(&q)) {
            continue;
        }
        let day = day_of(&e.s.started_at);
        match out.last_mut() {
            Some((d, items)) if *d == day => items.push(e),
            _ => out.push((day, vec![e])),
        }
    }
    out
}

/// Only what the search shows can be selected, so nothing hidden is deleted.
fn visible(app: &App) -> Vec<&Entry> {
    let st = &app.history;
    let filter = effective(&st.filter, &app.projects);
    days(st.sessions.as_deref().unwrap_or(&[]), &st.query, &filter, &app.projects)
        .into_iter()
        .flat_map(|(_, items)| items)
        .collect()
}

fn chosen(app: &App) -> Vec<String> {
    visible(app).into_iter().filter(|e| app.history.selected.contains(&e.s.id)).map(|e| e.s.id.clone()).collect()
}

/// Reloads the list.
pub fn reload(app: &App) -> Task {
    call(app, ops::list_sessions, |list| msg(Msg::Loaded(list)))
}

/// History is shown: its list reloads, and the search takes the keys.
pub fn on_open(app: &mut App) -> Task {
    app.history.today = Some(Local::now().date_naive());
    let focus = if app.view == View::History { task::focus(SEARCH) } else { Task::none() };
    Task::batch([reload(app), focus])
}

pub fn stop_selecting(app: &mut App) {
    app.history.selecting = false;
    app.history.selected.clear();
}

/// Shows one project's page.
pub fn show_project(app: &mut App, id: String) {
    app.history.filter = Filter::Project(id);
}

/// A session's title, if the list holds it.
pub fn title_of(app: &App, id: &str) -> Option<String> {
    app.history.sessions.as_ref()?.iter().find(|e| e.s.id == id).map(|e| e.s.title.clone())
}

pub fn update(app: &mut App, m: Msg) -> Task {
    let st = &mut app.history;
    match m {
        Msg::Loaded(list) => {
            let ids: HashSet<&str> = list.iter().map(|s| s.id.as_str()).collect();
            st.selected.retain(|id| ids.contains(id.as_str()));
            st.sessions = Some(list.into_iter().map(Entry::new).collect());
            Task::none()
        }
        Msg::Query(q) => {
            st.query = q;
            Task::none()
        }
        // Escape clears the search first, then leaves selection mode.
        Msg::Escape => {
            if !st.query.is_empty() {
                st.query.clear();
            } else if st.selecting {
                stop_selecting(app);
            }
            Task::none()
        }
        Msg::ToggleSelecting => {
            if st.selecting {
                stop_selecting(app);
            } else {
                st.selecting = true;
            }
            Task::none()
        }
        Msg::Open(id) => {
            if st.selecting {
                if !st.selected.remove(&id) {
                    st.selected.insert(id);
                }
                Task::none()
            } else {
                Task::done(Message::OpenSession(id))
            }
        }
        Msg::RowMenu(id) => {
            let list = app.history.sessions.as_deref().unwrap_or(&[]);
            let Some(e) = list.iter().find(|e| e.s.id == id) else { return Task::none() };
            let s = SessionRef { id: &e.s.id, recoverable: e.s.recoverable, project: e.s.project.as_deref() };
            Task::done(Message::Popup(session_menu::items(app, s, true), None))
        }
        Msg::FilterMenu => Task::done(Message::Popup(filter_menu(app), None)),
        Msg::PageMenu => Task::done(Message::Popup(
            vec![
                Item::action("history:edit-project", t("editProject")),
                Item::Separator,
                Item::action("history:delete-project", t("deleteProject")),
            ],
            None,
        )),
        Msg::ChooseAll => {
            let shown: Vec<String> = visible(app).iter().map(|e| e.s.id.clone()).collect();
            let all = !shown.is_empty() && shown.iter().all(|id| app.history.selected.contains(id));
            app.history.selected = if all { HashSet::new() } else { shown.into_iter().collect() };
            Task::none()
        }
        Msg::ChooseEmpty => {
            let empties = visible(app).into_iter().filter(|e| is_empty_session(&e.s)).map(|e| e.s.id.clone()).collect();
            app.history.selected = empties;
            Task::none()
        }
        Msg::ChosenProjectMenu => {
            let ids = chosen(app);
            if ids.is_empty() {
                return Task::none();
            }
            Task::done(Message::Popup(session_menu::project_items(app, &ids, None), None))
        }
        Msg::DeleteChosen => {
            let ids = chosen(app);
            if !ids.is_empty() {
                dialogs::ask_delete_selected(app, ids);
            }
            Task::none()
        }
    }
}

/// The project filter: all, each project, none, then a new project.
fn filter_menu(app: &App) -> Vec<Item> {
    let filter = effective(&app.history.filter, &app.projects);
    let mut items = vec![Item::check("history:filter-all", t("allProjects"), filter == Filter::All)];
    for p in &app.projects {
        let on = filter == Filter::Project(p.id.clone());
        items.push(Item::check(format!("history:filter:{}", p.id), menu_text(&p.name), on));
    }
    items.push(Item::check("history:filter-none", t("noProject"), filter == Filter::None));
    items.push(Item::Separator);
    items.push(Item::action("history:filter-new", t("newProjectMenu")));
    items
}

pub fn on_menu(app: &mut App, rest: &str) -> Task {
    let page = match effective(&app.history.filter, &app.projects) {
        Filter::Project(id) => known(&app.projects, Some(&id)).cloned(),
        _ => None,
    };
    match rest.split_once(':') {
        Some(("filter", id)) => app.history.filter = Filter::Project(id.to_string()),
        _ => match rest {
            "filter-all" => app.history.filter = Filter::All,
            "filter-none" => app.history.filter = Filter::None,
            // A new project opens as its own page, empty until sessions join it.
            "filter-new" => dialogs::ask_new_project_page(app),
            "edit-project" => {
                if let Some(p) = page {
                    dialogs::ask_edit_project(app, p);
                }
            }
            "delete-project" => {
                if let Some(p) = page {
                    dialogs::ask_delete_project(app, p);
                }
            }
            _ => {}
        },
    }
    Task::none()
}

/// Escape clears the search, or else leaves selection mode; a dialog takes it first.
pub fn on_key(app: &App, key: &KeyPress) -> Option<Message> {
    (app.view == View::History && key.code == "Escape").then_some(msg(Msg::Escape))
}

pub fn view(app: &App, ui: &mut Ui) -> El {
    let st = &app.history;
    let filter = effective(&st.filter, &app.projects);
    let page = match &filter {
        Filter::Project(id) => known(&app.projects, Some(id)),
        _ => None,
    };
    let all: &[Entry] = st.sessions.as_deref().unwrap_or(&[]);
    let days = days(all, &st.query, &filter, &app.projects);
    let shown: Vec<&Entry> = days.iter().flat_map(|(_, items)| items.iter().copied()).collect();

    let list: El = match &st.sessions {
        None => loading(),
        Some(list) if list.is_empty() => empty_history(),
        Some(_) if days.is_empty() => {
            let text =
                if page.is_some() && st.query.trim().is_empty() { t("projectEmpty") } else { t("historyNoMatch") };
            div()
                .w_full()
                .p(px(tokens::SPACE_8))
                .flex()
                .justify_center()
                .child(theme::t(text, tokens::BODY_SMALL, p().ink_muted).text_center())
                .into_any_element()
        }
        Some(_) => {
            let today = st.today.unwrap_or_else(|| Local::now().date_naive());
            let mut sections = col().px(px(10.0)).pb(px(tokens::SPACE_3));
            for (day, items) in &days {
                let mut rows = col().gap(px(2.0));
                for e in items {
                    rows = rows.child(row_view(app, e, page.is_some()));
                }
                sections = sections.child(col().child(day_header(day, today)).child(rows));
            }
            form::scroll(sections)
        }
    };

    let mut page_col = col()
        .size_full()
        .child(header(app, ui, page, &filter, &shown))
        .child(hairline())
        .child(div().flex_1().min_h_0().w_full().flex().flex_col().child(list));
    if st.selecting {
        page_col = page_col.child(hairline()).child(selection_bar(app, &shown));
    }
    page_col.into_any_element()
}

/// The title (or the project's square and name), its count, ⋯ and 選択; the search and the
/// project filter under it, and while selecting, the quick picks.
fn header(app: &App, ui: &mut Ui, page: Option<&Project>, filter: &Filter, shown: &[&Entry]) -> El {
    let st = &app.history;
    let sessions = st.sessions.as_deref().unwrap_or(&[]);
    let title: El = match page {
        // A long name wraps rather than pushing the count and the menu out of the window.
        Some(pr) => row()
            .gap(px(10.0))
            .min_w_0()
            .child(project_mark(Some(&pr.color), 14.0))
            .child(theme::t(pr.name.clone(), tokens::TITLE, p().ink).min_w_0())
            .into_any_element(),
        None => line(t("historyTitle"), tokens::TITLE, p().ink).into_any_element(),
    };
    let count = match page {
        Some(pr) => Some(sessions.iter().filter(|e| e.s.project.as_deref() == Some(pr.id.as_str())).count()),
        None => (!sessions.is_empty()).then_some(sessions.len()),
    };
    let mut top = div().flex().items_end().gap(px(10.0)).flex_1().min_w_0().child(title);
    if let Some(n) = count {
        top = top.child(line(tf("historyCount", &[("n", &n)]), tokens::CAPTION, p().ink_muted).pb(px(5.0)));
    }
    let mut actions = row().gap(px(4.0));
    if page.is_some() {
        actions = actions.child(icon_button(Icon::Ellipsis, t("projectMenu"), Size::Sm, Some(msg(Msg::PageMenu))));
    }
    if !sessions.is_empty() {
        let label = if st.selecting { t("done") } else { t("select") };
        actions = actions.child(btn(label, Variant::Ghost, Size::Sm, Some(msg(Msg::ToggleSelecting))));
    }
    let top = row().w_full().child(top).child(actions);

    let search =
        ui.input(Field::new(SEARCH, &st.query, |q| msg(Msg::Query(q))).placeholder(t("searchHistory")).search());
    let mut search_row = row().w_full().gap(px(6.0)).child(div().flex_1().min_w_0().child(search));
    if !app.projects.is_empty() {
        search_row = search_row.child(filter_button(page, filter));
    }

    let mut c = col().w_full().child(top).child(div().h(px(10.0))).child(search_row);
    if st.selecting {
        let all = !shown.is_empty() && shown.iter().all(|e| st.selected.contains(&e.s.id));
        let empties = shown.iter().filter(|e| is_empty_session(&e.s)).count();
        let mut picks = row()
            .gap(px(tokens::SPACE_4))
            .px(px(2.0))
            .pt(px(tokens::SPACE_2))
            .child(link(if all { t("selectNone") } else { t("selectAll") }, msg(Msg::ChooseAll)));
        if empties > 0 {
            picks = picks.child(link(tf("selectEmpty", &[("n", &empties)]), msg(Msg::ChooseEmpty)));
        }
        c = c.child(picks);
    }
    div().w_full().flex_none().px(px(tokens::SPACE_4)).py(px(tokens::SPACE_3)).child(c).into_any_element()
}

/// The project filter beside the search, shown once a project exists: what it narrows to, in ink
/// with its square when it narrows.
fn filter_button(page: Option<&Project>, filter: &Filter) -> El {
    let narrowed = *filter != Filter::All;
    let name = match (page, filter) {
        (Some(p), _) => p.name.clone(),
        (None, Filter::None) => t("noProject"),
        _ => t("project"),
    };
    let mut content = row().gap(px(6.0)).min_w_0();
    if narrowed {
        content = content.child(project_mark(page.map(|p| p.color.as_str()), 10.0));
    }
    let text = plain(name, tokens::CAPTION).min_w_0().overflow_hidden().text_ellipsis();
    let text = if narrowed { text.medium() } else { text };
    content = content.child(text).child(icon(Icon::ChevronDown, 13.0, p().ink_muted));
    let pal = p();
    let strong = pal.border_strong;
    let b = clickable(Some(msg(Msg::FilterMenu)), "filter")
        .flex()
        .flex_none()
        .items_center()
        .max_w(px(140.0))
        .h(px(38.0))
        .px(px(10.0))
        .border_1()
        .border_color(if narrowed { pal.ink } else { pal.border })
        .rounded(px(tokens::RADIUS_MD))
        .text_color(if narrowed { pal.ink } else { pal.ink_muted })
        .child(content);
    if narrowed { b } else { b.hover(move |s| s.border_color(strong)) }.into_any_element()
}

/// A quiet text button: muted, ink when pointed at.
fn link(label: String, on: Message) -> El {
    let ink = p().ink;
    clickable(Some(on), ("link", &label))
        .text_color(p().ink_muted)
        .hover(move |s| s.text_color(ink))
        .child(plain(label, tokens::CAPTION).medium())
        .into_any_element()
}

/// 今日 or 昨日 with the date, or the date alone.
fn day_header(day: &str, today: NaiveDate) -> El {
    let name = day_name(day, today).map(|d| match d {
        DayName::Today => t("today"),
        DayName::Yesterday => t("yesterday"),
    });
    let date = day_text(day, today.year());
    let mut r = row().gap(px(tokens::SPACE_2)).pt(px(14.0)).pb(px(tokens::SPACE_1)).px(px(tokens::SPACE_2));
    match name {
        Some(name) => {
            r = r.child(line(name, tokens::CAPTION, p().ink).medium()).child(line(
                date,
                tokens::CAPTION,
                p().ink_muted,
            ));
        }
        None => r = r.child(line(date, tokens::CAPTION, p().ink).medium()),
    }
    r.into_any_element()
}

/// One past session: its title (after its project's square, unless the page is that project's),
/// time, line, opening words and length. While selecting, a tick leads and a click ticks it.
fn row_view(app: &App, e: &Entry, in_project: bool) -> El {
    let st = &app.history;
    let s = &e.s;
    let empty = is_empty_session(s);
    let checked = st.selecting && st.selected.contains(&s.id);
    let ink = if empty { p().ink_muted } else { p().ink };
    let group: SharedString = format!("history-row-{}", s.id).into();

    let mut title = row().gap(px(tokens::SPACE_2)).flex_1().min_w_0().overflow_hidden();
    if !in_project && let Some(project) = known(&app.projects, s.project.as_deref()) {
        title = title.child(with_tip(project_mark(Some(&project.color), 10.0), project.name.clone()));
    }
    let style = if empty { tokens::BODY_SMALL } else { tokens::LABEL };
    title = title.child(line(s.title.clone(), style, ink).min_w_0());
    if s.recoverable {
        title = title.child(
            line(t("needsRecovery"), tokens::CAPTION, p().warning)
                .medium()
                .flex_none()
                .px(px(6.0))
                .bg(p().warning_tint)
                .rounded(px(tokens::RADIUS_SM)),
        );
    }
    // The time gives way to the row's ⋯ while the row is pointed at.
    let mut time = line(time_of(&s.started_at), tokens::CAPTION, p().ink_muted).mono().flex_none();
    if !st.selecting {
        time = time.group_hover(group.clone(), |s| s.invisible());
    }
    let top = row().gap(px(10.0)).w_full().child(title).child(time);

    let preview = if s.preview.is_empty() { t("emptyTranscript") } else { s.preview.clone() };
    let bottom = row()
        .gap(px(10.0))
        .w_full()
        .child(line(preview, tokens::CAPTION, p().ink_muted).flex_1().min_w_0())
        .child(line(duration_text(s.duration_ms), tokens::CAPTION, p().ink_muted).flex_none());

    let mut body = col().flex_1().min_w_0().child(top);
    if !empty {
        body =
            body.child(div().w_full().h(px(30.0)).pt(px(6.0)).pb(px(4.0)).child(session_line::small(&e.shape, e.two)));
    }
    body = body.child(bottom);

    let mut content = row().gap(px(tokens::SPACE_3)).w_full();
    if st.selecting {
        content = content.child(tick(checked));
    }
    content = content.child(body);

    let sunk = p().surface_sunken;
    let mut row_button = clickable(Some(msg(Msg::Open(s.id.clone()))), ("history-row", &s.id))
        .group(group.clone())
        .relative()
        .w_full()
        .pt(px(10.0))
        .pb(px(tokens::SPACE_2))
        .px(px(tokens::SPACE_2))
        .rounded(px(tokens::RADIUS_LG))
        .text_color(p().ink)
        .hover(move |s| s.bg(sunk))
        .child(content);
    if checked {
        row_button = row_button.bg(sunk);
    }
    if !st.selecting {
        let more = icon_button(Icon::Ellipsis, t("moreActions"), Size::Sm, Some(msg(Msg::RowMenu(s.id.clone()))));
        row_button = row_button.child(
            div().absolute().top(px(6.0)).right(px(6.0)).invisible().group_hover(group, |s| s.visible()).child(more),
        );
    }
    row_button.into_any_element()
}

/// A row's checkbox in selection mode: a rounded square, ticked in ink.
fn tick(on: bool) -> El {
    let pal = p();
    let mut b = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(18.0))
        .rounded(px(4.0))
        .border(px(1.5))
        .border_color(if on { pal.ink } else { pal.border_strong })
        .bg(if on { pal.ink } else { pal.surface_raised });
    if on {
        b = b.child(icon(Icon::Check, 12.0, pal.surface));
    }
    b.into_any_element()
}

/// While the list loads: three quiet blocks where rows will be.
fn loading() -> El {
    let block = || div().w_full().h(px(86.0)).bg(p().surface_sunken).rounded(px(tokens::RADIUS_LG));
    col()
        .gap(px(tokens::SPACE_2))
        .px(px(14.0))
        .pb(px(14.0))
        .pt(px(tokens::SPACE_3))
        .child(block())
        .child(block())
        .child(block())
        .into_any_element()
}

fn empty_history() -> El {
    div()
        .flex_1()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(tokens::SPACE_3))
        .pb(px(40.0))
        .child(icon(Icon::History, 26.0, p().ink_muted))
        .child(theme::t(t("historyEmpty"), tokens::BODY_SMALL, p().ink_muted))
        .into_any_element()
}

/// Under the list while selecting: how many are chosen, then プロジェクト and 削除 for them.
fn selection_bar(app: &App, shown: &[&Entry]) -> El {
    let n = shown.iter().filter(|e| app.history.selected.contains(&e.s.id)).count();
    let some = n > 0;
    // Under a dialog the dialog holds the view's primary.
    let delete = if dialogs::is_open(app) { Variant::Secondary } else { Variant::Primary };
    row()
        .w_full()
        .flex_none()
        .gap(px(tokens::SPACE_2))
        .py(px(10.0))
        .px(px(18.0))
        .child(line(tf("selectedCount", &[("n", &n)]), tokens::CAPTION, p().ink_muted).flex_1())
        .child(btn(t("project"), Variant::Secondary, Size::Lg, some.then(|| msg(Msg::ChosenProjectMenu))))
        .child(btn(t("delete"), delete, Size::Lg, some.then(|| msg(Msg::DeleteChosen))))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use chidori_core::session::recovery::Activity;

    use super::*;

    fn s(id: &str, started_at: &str, title: &str, preview: &str) -> SessionSummary {
        SessionSummary {
            id: id.into(),
            title: title.into(),
            started_at: started_at.into(),
            duration_ms: 60_000,
            folder: String::new(),
            segments: 3,
            screenshots: 0,
            recoverable: false,
            sources: vec![SourceId::App, SourceId::Mic],
            preview: preview.into(),
            activity: Activity { others: vec![], me: vec![], shots: vec![] },
            project: None,
        }
    }

    fn ids<'a>(days: &[(&'a str, Vec<&'a Entry>)]) -> Vec<(&'a str, Vec<&'a str>)> {
        days.iter().map(|(d, items)| (*d, items.iter().map(|e| e.s.id.as_str()).collect())).collect()
    }

    #[test]
    fn groups_by_day_in_order() {
        let list: Vec<Entry> = [
            s("a", "2026-10-03T15:00:00+09:00", "A", ""),
            s("b", "2026-10-03T09:00:00+09:00", "B", ""),
            s("c", "2026-10-02T20:00:00+09:00", "C", ""),
        ]
        .into_iter()
        .map(Entry::new)
        .collect();
        assert_eq!(
            ids(&days(&list, "", &Filter::All, &[])),
            [("2026-10-03", vec!["a", "b"]), ("2026-10-02", vec!["c"])]
        );
    }

    #[test]
    fn searches_titles_and_opening_words_in_any_case_and_width() {
        let list: Vec<Entry> = [
            s("a", "2026-10-03T15:00:00+09:00", "Zoom 定例", "来週の予定"),
            s("b", "2026-10-03T09:00:00+09:00", "講義", "ＧＰＵの話"),
        ]
        .into_iter()
        .map(Entry::new)
        .collect();
        let found =
            |q: &str| ids(&days(&list, q, &Filter::All, &[])).into_iter().flat_map(|(_, i)| i).collect::<Vec<_>>();
        assert_eq!(found("zoom"), ["a"]);
        assert_eq!(found(" 予定 "), ["a"]);
        assert_eq!(found("gpu"), ["b"]);
        assert_eq!(found("nothing"), Vec::<&str>::new());
    }

    #[test]
    fn filters_by_project_and_forgets_a_deleted_one() {
        let projects = [Project { id: "p1".into(), name: "P".into(), color: "blue".into() }];
        let mut a = s("a", "2026-10-03T15:00:00+09:00", "A", "");
        a.project = Some("p1".into());
        let mut b = s("b", "2026-10-03T09:00:00+09:00", "B", "");
        b.project = Some("gone".into());
        let list: Vec<Entry> =
            [a, b, s("c", "2026-10-03T08:00:00+09:00", "C", "")].into_iter().map(Entry::new).collect();
        let found = |f: &Filter| {
            ids(&days(&list, "", &effective(f, &projects), &projects))
                .into_iter()
                .flat_map(|(_, i)| i)
                .collect::<Vec<_>>()
        };
        assert_eq!(found(&Filter::Project("p1".into())), ["a"]);
        // A session whose project was deleted is in none.
        assert_eq!(found(&Filter::None), ["b", "c"]);
        assert_eq!(found(&Filter::Project("gone".into())), ["a", "b", "c"]);
    }

    #[test]
    fn short_and_empty_sessions_are_greyed() {
        let mut short = s("a", "2026-10-03T15:00:00+09:00", "A", "");
        short.duration_ms = 9_000;
        let mut silent = s("b", "2026-10-03T15:00:00+09:00", "B", "");
        silent.segments = 0;
        assert!(is_empty_session(&short));
        assert!(is_empty_session(&silent));
        assert!(!is_empty_session(&s("c", "2026-10-03T15:00:00+09:00", "C", "")));
    }

    #[test]
    fn a_session_with_both_sides_draws_two() {
        assert!(Entry::new(s("a", "2026-10-03T15:00:00+09:00", "A", "")).two);
        let mut mic = s("b", "2026-10-03T15:00:00+09:00", "B", "");
        mic.sources = vec![SourceId::Mic];
        assert!(!Entry::new(mic).two);
    }
}
