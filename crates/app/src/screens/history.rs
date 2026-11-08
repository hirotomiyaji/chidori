//! 履歴: past sessions by day, each day a white panel of rows; searchable by title and opening
//! words (FR-62, FR-63), and narrowed to one project by its chip (FR-64), which makes the page that
//! project's: its name on top and its own menu to change or delete it. 選択 picks several to move to the Recycle
//! Bin at once, such as all the short and empty ones, or to put in a project.

use std::collections::HashSet;

use chidori_core::ops;
use chidori_core::session::projects::Project;
use chidori_core::session::recovery::SessionSummary;
use chrono::{Datelike, Local, NaiveDate};
use gpui_kit::{InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled, div, px};

use super::dialogs::{self, project_mark};
use super::form;
use super::session_menu::{self, SessionRef};
use crate::app::{App, Message, View, call};
use crate::i18n::{t, tf};
use crate::icons::{Icon, icon};
use crate::model::format::{DayName, day_name, day_of, day_text, duration_text, time_of};
use crate::native::menu::Item;
use crate::task::{self, Task};
use crate::theme::{self, GRID, Ink, R_PANEL, R_ROW, R_TAG, g, line, p, project_color, tokens};
use crate::ui::{El, Field, KeyPress, Ui, clickable};
use crate::widgets::{
    Size, Variant, btn, chip, chips, col, fill, group_label, icon_button, panel, row, text_button, with_tip,
};

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

/// A row: the session as the list loaded it.
#[derive(Debug, Clone)]
struct Entry {
    s: SessionSummary,
}

impl Entry {
    fn new(s: SessionSummary) -> Self {
        Entry { s }
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
    /// The project's own menu, on its page.
    PageMenu,
    /// Selects every row shown, or none once all are.
    ChooseAll,
    ChooseEmpty,
    /// A chip: the same as the pick of that name in the project menu.
    Pick(String),
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
        Msg::Pick(rest) => on_menu(app, &rest),
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
                .p(g(4.0))
                .flex()
                .justify_center()
                .child(theme::t(text, tokens::BODY, p().muted).text_center())
                .into_any_element()
        }
        Some(_) => {
            let today = st.today.unwrap_or_else(|| Local::now().date_naive());
            let mut sections = col().gap(g(1.0)).px(g(2.0)).pb(g(2.0));
            for (day, items) in &days {
                let rows = items.iter().map(|e| row_view(app, e, page.is_some())).collect();
                sections = sections.child(day_header(day, today)).child(panel(rows));
            }
            form::scroll(sections)
        }
    };

    let mut page_col = col()
        .size_full()
        .child(header(app, ui, page, &filter, &shown))
        .child(div().flex_1().min_h_0().w_full().flex().flex_col().child(list));
    if st.selecting {
        page_col = page_col.child(selection_bar(app, &shown));
    }
    page_col.into_any_element()
}

/// The search pill, the project chips with 選択 after them, and while selecting, the quick picks.
/// On a project's page, its name and menu come first.
fn header(app: &App, ui: &mut Ui, page: Option<&Project>, filter: &Filter, shown: &[&Entry]) -> El {
    let st = &app.history;
    let sessions = st.sessions.as_deref().unwrap_or(&[]);
    let mut c = col().w_full().flex_none().gap(g(1.0)).px(g(2.0)).pt(g(1.0)).pb(g(1.0));

    if let Some(pr) = page {
        let n = sessions.iter().filter(|e| e.s.project.as_deref() == Some(pr.id.as_str())).count();
        // A long name wraps rather than pushing the count and the menu out of the window.
        let title = row()
            .gap(g(1.0))
            .flex_1()
            .min_w_0()
            .child(project_mark(Some(&pr.color), GRID))
            .child(theme::t(pr.name.clone(), tokens::HEADING, p().ink).min_w_0())
            .child(line(tf("historyCount", &[("n", &n)]), tokens::SMALL, p().muted).flex_none());
        c = c.child(row().w_full().pl(g(2.0)).child(title).child(icon_button(
            Icon::Ellipsis,
            t("projectMenu"),
            Size::Md,
            Some(msg(Msg::PageMenu)),
        )));
    }

    let search =
        ui.input(Field::new(SEARCH, &st.query, |q| msg(Msg::Query(q))).placeholder(t("searchHistory")).search());
    c = c
        .child(row().w_full().h(g(6.0)).px(g(2.0)).rounded(g(3.0)).bg(p().surface).child(div().flex_1().child(search)));

    let mut chips_row = row().w_full().gap(g(1.0));
    if !app.projects.is_empty() || matches!(filter, Filter::None) {
        let mut items =
            vec![chip(t("allChip"), None, *filter == Filter::All, Some(msg(Msg::Pick("filter-all".into()))))];
        let mut chosen = (*filter == Filter::All).then_some(0);
        for pr in &app.projects {
            let on = *filter == Filter::Project(pr.id.clone());
            if on {
                chosen = Some(items.len());
            }
            items.push(chip(
                pr.name.clone(),
                Some(project_color(&pr.color)),
                on,
                Some(msg(Msg::Pick(format!("filter:{}", pr.id)))),
            ));
        }
        if *filter == Filter::None {
            chosen = Some(items.len());
        }
        items.push(chip(t("noProject"), None, *filter == Filter::None, Some(msg(Msg::Pick("filter-none".into())))));
        let strip =
            div().id("history-chips").flex_1().min_w_0().overflow_x_scroll().child(chips("history", items, chosen));
        chips_row = chips_row.child(strip);
    } else {
        chips_row = chips_row.child(fill());
    }
    chips_row =
        chips_row.child(icon_button(Icon::Plus, t("newProject"), Size::Md, Some(msg(Msg::Pick("filter-new".into())))));
    if !sessions.is_empty() {
        let label = if st.selecting { t("done") } else { t("select") };
        chips_row = chips_row.child(btn(label, Variant::Ghost, Size::Sm, Some(msg(Msg::ToggleSelecting))));
    }
    c = c.child(chips_row);

    if st.selecting {
        let all = !shown.is_empty() && shown.iter().all(|e| st.selected.contains(&e.s.id));
        let empties = shown.iter().filter(|e| is_empty_session(&e.s)).count();
        let mut picks = row().gap(g(1.0)).child(text_button(
            None,
            if all { t("selectNone") } else { t("selectAll") },
            Some(msg(Msg::ChooseAll)),
        ));
        if empties > 0 {
            picks = picks.child(text_button(None, tf("selectEmpty", &[("n", &empties)]), Some(msg(Msg::ChooseEmpty))));
        }
        c = c.child(picks);
    }
    c.into_any_element()
}

/// 今日 or 昨日 with the date, or the date alone, over the day's panel.
fn day_header(day: &str, today: NaiveDate) -> El {
    let name = day_name(day, today).map(|d| match d {
        DayName::Today => t("today"),
        DayName::Yesterday => t("yesterday"),
    });
    let date = day_text(day, today.year());
    let text = match name {
        Some(name) => format!("{name} · {date}"),
        None => date,
    };
    div().pt(g(1.0)).child(group_label(text)).into_any_element()
}

/// 「22:00 · 41分 · 3枚のスクショ」.
fn row_meta(s: &SessionSummary) -> String {
    let mut parts = vec![time_of(&s.started_at), duration_text(s.duration_ms)];
    if s.screenshots > 0 {
        parts.push(tf("shotsCount", &[("n", &s.screenshots)]));
    }
    parts.join(" · ")
}

/// One past session, 64 high: its project's dot and title over when and how long, then ⋯. While
/// selecting, a tick leads and a click ticks it. A short or empty one is in grey.
fn row_view(app: &App, e: &Entry, in_project: bool) -> El {
    let st = &app.history;
    let s = &e.s;
    let empty = is_empty_session(s);
    let checked = st.selecting && st.selected.contains(&s.id);
    let ink = if empty { p().muted } else { p().ink };

    let mut title = row().gap(g(1.0)).w_full().min_w_0().overflow_hidden();
    if !in_project && let Some(project) = known(&app.projects, s.project.as_deref()) {
        title = title.child(with_tip(project_mark(Some(&project.color), GRID), project.name.clone()));
    }
    title = title.child(line(s.title.clone(), tokens::BODY, ink).min_w_0());
    if s.recoverable {
        title = title.child(
            line(t("needsRecovery"), tokens::SMALL, p().on_ink)
                .bold()
                .flex_none()
                .px(g(1.0))
                .bg(p().ink)
                .rounded(px(R_TAG)),
        );
    }
    let mut meta = row_meta(s);
    if !s.preview.is_empty() && !empty {
        meta = format!("{meta} · {}", s.preview);
    }
    let body = col().flex_1().min_w_0().child(title).child(line(meta, tokens::SMALL, p().muted).min_w_0());

    let mut content = row().gap(g(2.0)).w_full();
    if st.selecting {
        content = content.child(tick(checked));
    }
    content = content.child(body);
    if !st.selecting {
        content = content.child(icon_button(
            Icon::Ellipsis,
            t("moreActions"),
            Size::Md,
            Some(msg(Msg::RowMenu(s.id.clone()))),
        ));
    }

    let hover = p().hover_panel;
    let mut row_button = clickable(Some(msg(Msg::Open(s.id.clone()))), ("history-row", &s.id))
        .flex()
        .items_center()
        .w_full()
        .h(g(8.0))
        .pl(g(2.0))
        .pr(g(1.0))
        .rounded(px(R_ROW))
        .hover(move |s| s.bg(hover))
        .child(content);
    if checked {
        row_button = row_button.bg(hover);
    }
    row_button.into_any_element()
}

/// A row's checkbox in selection mode: a round tick, ink when on.
fn tick(on: bool) -> El {
    let pal = p();
    let mut b = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(g(3.0))
        .rounded(g(1.5))
        .border_2()
        .border_color(if on { pal.ink } else { pal.line })
        .bg(if on { pal.ink } else { pal.surface });
    if on {
        b = b.child(icon(Icon::Check, 16.0, pal.on_ink));
    }
    b.into_any_element()
}

/// While the list loads: quiet panels where the days will be.
fn loading() -> El {
    let block = || div().w_full().h(g(17.0)).bg(p().surface).rounded(px(R_PANEL));
    col().gap(g(1.0)).px(g(2.0)).pt(g(4.0)).child(block()).child(block()).into_any_element()
}

fn empty_history() -> El {
    div()
        .flex_1()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(g(2.0))
        .pb(g(5.0))
        .child(icon(Icon::History, 24.0, p().muted))
        .child(theme::t(t("historyEmpty"), tokens::BODY, p().muted))
        .into_any_element()
}

/// The bar while selecting: how many are chosen, then プロジェクト and 削除 for them.
fn selection_bar(app: &App, shown: &[&Entry]) -> El {
    let n = shown.iter().filter(|e| app.history.selected.contains(&e.s.id)).count();
    let some = n > 0;
    // Under a dialog the dialog holds the view's primary.
    let delete = if dialogs::is_open(app) { Variant::Secondary } else { Variant::Primary };
    row()
        .w_full()
        .flex_none()
        .h(g(10.0))
        .px(g(2.0))
        .pb(g(1.0))
        .gap(g(1.0))
        .child(line(tf("selectedCount", &[("n", &n)]), tokens::SMALL, p().muted).pl(g(2.0)).flex_1())
        .child(btn(t("project"), Variant::Secondary, Size::Lg, some.then(|| msg(Msg::ChosenProjectMenu))))
        .child(btn(t("delete"), delete, Size::Lg, some.then(|| msg(Msg::DeleteChosen))))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use chidori_core::session::model::SourceId;
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
}
