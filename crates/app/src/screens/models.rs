//! The model list (section 12): each model with its size, licence and speed on this PC, to
//! download, use, measure or delete. The recommended ones show first; the others wait behind
//! ほかのモデル. 設定 › モデル shows it (`list`).
//! Also the quiet model note under the deck (`note`), which opens the list when idle.

use std::collections::{HashMap, HashSet};

use chidori_core::asr::worker::EngineState;
use chidori_core::error::AppError;
use chidori_core::events::DownloadPayload;
use chidori_core::models::benchmark::{BenchmarkResult, Tier};
use chidori_core::models::manager::ModelEntry;
use chidori_core::ops;
use gpui_kit::{IntoElement, ParentElement, Styled, div, px};

use super::form;
use crate::actions::{report_error, with_parent};
use crate::app::{App, Level, Message, SettingsTab, call};
use crate::i18n::{is_ja, t, tf};
use crate::icons::{Icon, icon};
use crate::model::format::{bytes, model_name_parts};
use crate::task::Task;
use crate::theme::{self, Ink, g, line, p, tokens};
use crate::ui::El;
use crate::widgets::{self, Size, Variant, btn, btn_icon, col, fill, icon_button, progress, row, spinner, with_tip};

/// The models the app recommends (section 12); the others wait behind ほかのモデル.
pub const MAIN_MODELS: [&str; 2] = ["turbo-q5", "small-q5"];

#[derive(Debug, Default)]
pub struct State {
    /// The latest download progress per model, kept while the app runs.
    pub progress: HashMap<String, DownloadPayload>,
    /// Speed tests run since the app started, newer than the stored records.
    bench: HashMap<String, Bench>,
    benching: HashSet<String>,
    more: bool,
    import_open: bool,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Download(String),
    Cancel(String),
    Use(String),
    Used(String, Result<(), AppError>),
    Delete(String),
    Benchmark(String),
    Benchmarked(String, Result<BenchmarkResult, AppError>),
    /// A command finished; the list rereads either way.
    Done(Result<(), AppError>),
    More,
    ToggleImport,
    Import(String),
    Imported(Result<(), AppError>),
    RetryGpu,
}

/// A speed test's result: 10 s of audio in `seconds`, and how that feels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bench {
    pub seconds: f32,
    /// `comfortable`, `ok` or `heavy`, as the core stores it.
    pub tier: &'static str,
}

impl Bench {
    pub fn heavy(&self) -> bool {
        self.tier == "heavy"
    }

    /// `10秒の音声を 2.5 秒で処理（快適）`.
    pub fn text(&self) -> String {
        let tier = t(match self.tier {
            "comfortable" => "tierComfortable",
            "ok" => "tierOk",
            _ => "tierHeavy",
        });
        tf("benchmarkResult", &[("seconds", &format!("{:.1}", self.seconds)), ("tier", &tier)])
    }
}

impl From<&BenchmarkResult> for Bench {
    fn from(b: &BenchmarkResult) -> Self {
        let tier = match b.tier {
            Tier::Comfortable => "comfortable",
            Tier::Ok => "ok",
            Tier::Heavy => "heavy",
        };
        Bench { seconds: b.seconds, tier }
    }
}

fn stored_bench(m: &ModelEntry) -> Option<Bench> {
    let r = m.benchmark.as_ref()?;
    let tier = match r.tier.as_str() {
        "comfortable" => "comfortable",
        "ok" => "ok",
        _ => "heavy",
    };
    Some(Bench { seconds: r.seconds, tier })
}

/// A catalog model's name in the UI language.
pub fn name_of(m: &ModelEntry) -> &str {
    if is_ja() { &m.entry.name_ja } else { &m.entry.name_en }
}

pub fn description_of(m: &ModelEntry) -> &str {
    if is_ja() { &m.entry.description_ja } else { &m.entry.description_en }
}

/// Everyday and technical name of a catalog model (`標準`, `large-v3-turbo q5_0`) and the two
/// together as the catalog writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelName {
    pub short: String,
    pub tech: String,
    pub full: String,
}

pub fn model_name(models: &[ModelEntry], id: &str) -> Option<ModelName> {
    if id.is_empty() {
        return None;
    }
    let Some(m) = models.iter().find(|m| m.entry.id == id) else {
        return Some(ModelName { short: id.to_string(), tech: String::new(), full: id.to_string() });
    };
    let full = name_of(m).to_string();
    let (short, tech) = model_name_parts(&full);
    Some(ModelName { short, tech, full })
}

fn downloading(m: &ModelEntry, p: Option<&DownloadPayload>) -> bool {
    m.downloading || p.is_some_and(|p| matches!(p.phase, "downloading" | "verifying"))
}

/// The models shown, the recommended ones and what the user has or is getting first: a model in
/// use or on its way never hides behind ほかのモデル. Opening the rest adds them below, so
/// nothing already in view moves. Returns them and how many still wait.
pub fn shown<'a>(
    models: &'a [ModelEntry],
    progress: &HashMap<String, DownloadPayload>,
    more: bool,
) -> (Vec<&'a ModelEntry>, usize) {
    let visible = models.iter().filter(|m| !m.entry.hidden || m.installed);
    let main = |m: &ModelEntry| {
        MAIN_MODELS.contains(&m.entry.id.as_str())
            || m.installed
            || m.selected
            || m.downloading
            || progress.contains_key(&m.entry.id)
    };
    let (first, others): (Vec<_>, Vec<_>) = visible.partition(|m| main(m));
    let waiting = others.len();
    if more { (first.into_iter().chain(others).collect(), 0) } else { (first, waiting) }
}

/// Reports a failed command, then rereads the list.
fn done(app: &mut App, r: Result<(), AppError>) -> Task {
    if let Err(e) = r {
        report_error(app, &e, "failed");
    }
    app.refresh_models()
}

fn to_done(r: Result<(), AppError>) -> Message {
    Message::ModelsUi(Msg::Done(r))
}

pub fn update(app: &mut App, msg: Msg) -> Task {
    match msg {
        Msg::Download(id) => call(app, move |c| ops::model_download(c, &id), to_done),
        Msg::Cancel(id) => call(
            app,
            move |c| {
                ops::model_cancel(c, &id);
                Ok(())
            },
            to_done,
        ),
        Msg::Use(id) => {
            let picked = id.clone();
            call(app, move |c| ops::model_select(c, &id), move |r| Message::ModelsUi(Msg::Used(picked, r)))
        }
        Msg::Used(id, r) => {
            if r.is_ok() {
                // The core saved it; the summary in 設定 reads it from here.
                app.settings.model_id = id;
            }
            done(app, r)
        }
        Msg::Delete(id) => call(app, move |c| ops::model_delete(c, &id), to_done),
        Msg::Benchmark(id) => {
            app.models_ui.benching.insert(id.clone());
            let tested = id.clone();
            call(app, move |c| ops::run_benchmark(c, &id), move |r| Message::ModelsUi(Msg::Benchmarked(tested, r)))
        }
        Msg::Benchmarked(id, r) => {
            app.models_ui.benching.remove(&id);
            match r {
                Ok(b) => {
                    app.models_ui.bench.insert(id, Bench::from(&b));
                }
                Err(e) => report_error(app, &e, "failed"),
            }
            app.refresh_models()
        }
        Msg::Done(r) => done(app, r),
        Msg::More => {
            app.models_ui.more = true;
            Task::none()
        }
        Msg::ToggleImport => {
            app.models_ui.import_open = !app.models_ui.import_open;
            Task::none()
        }
        Msg::Import(id) => {
            let parent = app.hwnd;
            call(
                app,
                move |c| {
                    let dialog = rfd::FileDialog::new().add_filter("Whisper GGML", &["bin"]);
                    let path = with_parent(dialog, parent).pick_file()?;
                    Some(ops::model_import(c, &id, &path))
                },
                |r| r.map_or(Message::Noop, |r| Message::ModelsUi(Msg::Imported(r))),
            )
        }
        Msg::Imported(r) => {
            match r {
                Ok(()) => app.toast(t("saved"), Level::Info),
                Err(e) => report_error(app, &e, "failed"),
            }
            app.refresh_models()
        }
        Msg::RetryGpu => {
            app.toast(t("loading"), Level::Info);
            call(app, ops::retry_gpu, |_| Message::Noop)
        }
    }
}

/// Download progress: kept per model, and the list rereads once a download stops.
pub fn on_download(app: &mut App, p: &DownloadPayload) -> Task {
    app.models_ui.progress.insert(p.id.clone(), p.clone());
    if p.phase == "error"
        && let Some(e) = &p.error
    {
        report_error(app, e, "failed");
    }
    if matches!(p.phase, "downloading" | "verifying") { Task::none() } else { app.refresh_models() }
}

pub fn spinning(app: &App) -> bool {
    !app.models_ui.benching.is_empty() && app.view == crate::app::View::Settings
}

/// The list itself, for the page and for 設定 › モデル.
pub fn list(app: &App) -> El {
    let s = &app.models_ui;
    let (models, waiting) = shown(&app.models, &s.progress, s.more);
    let mut c = col().w_full().gap(g(1.0));
    for m in models {
        c = c.child(model_row(app, m));
    }
    if waiting > 0 {
        c = c.child(form::link(
            row()
                .gap(px(4.0))
                .child(icon(Icon::ChevronDown, 13.0, p().muted))
                .child(theme::plain(tf("moreModels", &[("n", &waiting)]), tokens::SMALL).bold()),
            Message::ModelsUi(Msg::More),
        ));
    }
    c = c.child(import(app));
    if app.info.gpu_failed {
        c = c.child(btn(t("retryGpu"), Variant::Secondary, Size::Sm, Some(Message::ModelsUi(Msg::RetryGpu))));
    }
    div().w_full().px(g(2.0)).pt(g(1.0)).pb(g(2.0)).child(c).into_any_element()
}

/// ファイルから追加: a model downloaded elsewhere, one button per model not on this PC.
fn import(app: &App) -> El {
    let open = app.models_ui.import_open;
    let summary = form::link(
        row()
            .gap(px(4.0))
            .child(icon(if open { Icon::ChevronDown } else { Icon::ChevronRight }, 13.0, p().muted))
            .child(theme::plain(t("importFromFile"), tokens::SMALL)),
        Message::ModelsUi(Msg::ToggleImport),
    );
    let mut c = col().gap(px(6.0)).pt(g(1.0)).items_start().child(summary);
    if open {
        let mut buttons = div().flex().flex_wrap().gap(px(6.0));
        for m in app.models.iter().filter(|m| (!m.entry.hidden || m.installed) && !m.installed) {
            buttons = buttons.child(btn(
                m.entry.file.clone(),
                Variant::Secondary,
                Size::Sm,
                Some(Message::ModelsUi(Msg::Import(m.entry.id.clone()))),
            ));
        }
        c = c.child(buttons);
    }
    c.into_any_element()
}

fn model_row(app: &App, m: &ModelEntry) -> El {
    let s = &app.models_ui;
    let id = m.entry.id.clone();
    let prog = s.progress.get(&m.entry.id);
    let busy = app.rec.busy();
    let in_use = m.selected && m.installed;
    let getting = downloading(m, prog);

    let mut name = div().flex().flex_wrap().items_center().gap(px(6.0)).child(theme::t(
        name_of(m).to_string(),
        tokens::LABEL,
        p().ink,
    ));
    if m.recommended {
        name = name.child(form::tag(t("recommended"), false));
    }
    if in_use {
        name = name.child(form::tag(t("selected"), true));
    }
    let mut about = col().gap(px(2.0)).flex_1().min_w_0().child(name).child(theme::t(
        format!("{} · {} · {}", description_of(m), bytes(m.entry.size_bytes), m.entry.license),
        tokens::SMALL,
        p().muted,
    ));
    if let Some(b) = s.bench.get(&m.entry.id).copied().or_else(|| stored_bench(m)) {
        let mut text = b.text();
        if b.heavy() {
            text = format!("{text} — {}", t("tierHeavyHint"));
        }
        about = about.child(theme::t(text, tokens::SMALL, if b.heavy() { p().them } else { p().me }));
    }

    let mut actions = row().gap(px(4.0)).flex_none();
    if m.installed {
        if !m.selected {
            // Secondary: the list may hold several models to switch to, and a view has one primary.
            actions = actions.child(btn_icon(
                Icon::Check,
                t("use"),
                Variant::Secondary,
                Size::Md,
                (!busy).then(|| Message::ModelsUi(Msg::Use(id.clone()))),
            ));
        }
        let benching = s.benching.contains(&m.entry.id);
        let gauge: El = if benching {
            with_tip(
                div().flex().items_center().justify_center().size(px(32.0)).child(spinner(14.0, app.now)),
                t("benchmark"),
            )
            .into_any_element()
        } else {
            icon_button(
                Icon::Gauge,
                t("benchmark"),
                Size::Md,
                (!busy).then(|| Message::ModelsUi(Msg::Benchmark(id.clone()))),
            )
        };
        actions = actions.child(gauge);
        actions = actions.child(icon_button(
            Icon::Trash,
            t("delete"),
            Size::Md,
            (!(busy && m.selected)).then(|| Message::ModelsUi(Msg::Delete(id.clone()))),
        ));
    } else if getting {
        actions = actions.child(icon_button(
            Icon::X,
            t("cancel"),
            Size::Md,
            Some(Message::ModelsUi(Msg::Cancel(id.clone()))),
        ));
    } else {
        let label = if m.partial_bytes > 0 { t("resumeDownload") } else { t("download") };
        actions = actions.child(btn_icon(
            Icon::Download,
            label,
            Variant::Secondary,
            Size::Md,
            Some(Message::ModelsUi(Msg::Download(id))),
        ));
    }

    let mut body = col().w_full().child(div().flex().items_start().w_full().gap(g(1.0)).child(about).child(actions));
    if getting && let Some(p) = prog {
        body = body.child(div().h(g(1.0))).child(download_progress(p.received, p.total, p.bytes_per_sec));
    }
    form::card(body, in_use).into_any_element()
}

/// A download's bar, with how much has come and how fast.
pub fn download_progress(received: u64, total: u64, bytes_per_sec: f64) -> El {
    let mut numbers =
        row().w_full().child(line(format!("{} / {}", bytes(received), bytes(total)), tokens::SMALL, p().muted).mono());
    if bytes_per_sec > 0.0 {
        numbers = numbers.child(fill()).child(
            line(tf("downloadSpeed", &[("speed", &bytes(bytes_per_sec as u64))]), tokens::SMALL, p().muted).mono(),
        );
    }
    col().w_full().gap(px(4.0)).child(progress(received as f32, total as f32)).child(numbers).into_any_element()
}

/// Which model transcribes and on what, as a quiet line of text (`標準 · GPU`). Opens 設定 ›
/// モデル when idle; while recording it only says.
pub fn note(app: &App) -> Option<El> {
    let engine = app.rec.engine.as_ref();
    let id = match engine {
        Some(e) if e.state == EngineState::Ready && e.model_id.is_some() => e.model_id.clone().unwrap_or_default(),
        _ => app.settings.model_id.clone(),
    };
    let name = model_name(&app.models, &id)?;
    let busy = app.rec.busy();
    let loading = engine.is_some_and(|e| e.state == EngineState::Loading);
    let failed = engine.is_some_and(|e| e.state == EngineState::Failed);
    let device = engine.filter(|e| e.state == EngineState::Ready).map(|e| if e.gpu { t("gpu") } else { t("cpu") });
    let gpu_name = engine.filter(|e| e.gpu).and_then(|e| e.device_name.clone());
    let tip = [
        Some(name.full.clone()),
        gpu_name.or(device.clone()),
        loading.then(|| t("modelLoading")),
        (!busy).then(|| t("changeModel")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n");

    let mut body = row().gap(px(4.0)).child(line(name.short, tokens::SMALL, p().ink).bold());
    if let Some(d) = device {
        body = body.child(line(format!("· {d}"), tokens::SMALL, p().muted));
    }
    if loading {
        body = body.child(spinner(11.0, app.now));
    }
    if failed {
        body = body.child(line(format!("· {}", t("modelFailed")), tokens::SMALL, p().ink).bold());
    }
    let content: El = if busy {
        body.into_any_element()
    } else {
        widgets::bare(body, Some(Message::GoSettings(SettingsTab::Models)), false).into_any_element()
    };
    Some(with_tip(content, tip).into_any_element())
}

#[cfg(test)]
mod tests {
    use chidori_core::models::catalog;

    use super::*;

    pub fn entry(id: &str) -> ModelEntry {
        ModelEntry {
            entry: catalog::find(id).expect("in the catalog").clone(),
            installed: false,
            selected: false,
            recommended: false,
            partial_bytes: 0,
            downloading: false,
            benchmark: None,
        }
    }

    fn ids(v: &[&ModelEntry]) -> Vec<String> {
        v.iter().map(|m| m.entry.id.clone()).collect()
    }

    #[test]
    fn recommended_models_first_and_the_rest_behind_more() {
        let models: Vec<_> = catalog::all().iter().map(|e| entry(&e.id)).collect();
        let none = HashMap::new();
        let (first, waiting) = shown(&models, &none, false);
        assert_eq!(ids(&first), ["turbo-q5", "small-q5"]);
        let hidden = models.iter().filter(|m| m.entry.hidden).count();
        assert_eq!(waiting, models.len() - 2 - hidden);
        let (all, waiting) = shown(&models, &none, true);
        assert_eq!(&ids(&all)[..2], ["turbo-q5", "small-q5"]);
        assert_eq!(all.len(), models.len() - hidden);
        assert_eq!(waiting, 0);
    }

    #[test]
    fn a_model_one_has_or_is_getting_stays_in_view() {
        let mut models: Vec<_> = catalog::all().iter().map(|e| entry(&e.id)).collect();
        let other = models.iter().position(|m| !MAIN_MODELS.contains(&m.entry.id.as_str()) && !m.entry.hidden).unwrap();
        models[other].installed = true;
        let (first, _) = shown(&models, &HashMap::new(), false);
        assert!(ids(&first).contains(&models[other].entry.id));
    }

    #[test]
    fn names_split_into_everyday_and_technical() {
        crate::i18n::set_locale(chidori_core::settings::Locale::Ja);
        let mut m = entry("turbo-q5");
        m.entry.name_ja = "標準 (large-v3-turbo q5_0)".into();
        let name = model_name(&[m], "turbo-q5").unwrap();
        assert_eq!(name.short, "標準");
        assert_eq!(name.tech, "large-v3-turbo q5_0");
        assert_eq!(model_name(&[], "custom").unwrap().full, "custom");
        assert_eq!(model_name(&[], ""), None);
    }

    #[test]
    fn speed_test_says_how_it_feels() {
        crate::i18n::set_locale(chidori_core::settings::Locale::Ja);
        assert_eq!(Bench { seconds: 2.5, tier: "comfortable" }.text(), "10秒の音声を 2.5 秒で処理（快適）");
        assert!(Bench { seconds: 14.0, tier: "heavy" }.heavy());
    }
}
