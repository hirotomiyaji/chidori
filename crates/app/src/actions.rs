//! User actions shared by buttons, menus, dialogs and the tray, with error reporting.

use std::path::PathBuf;

use crate::task::Task;
use chidori_core::error::{AppError, ErrorCode};
use chidori_core::events::{Notice, NoticeLevel, UiState};
use chidori_core::ops::{self, CopyFormat, PdfExport};
use chidori_core::settings::SourceMode;

use crate::app::{App, Level, Message, SettingsTab, View, call};
use crate::i18n::{t, t_json, tf};

#[derive(Debug, Clone)]
pub enum Action {
    ToggleRecording,
    PauseResume,
    Stop,
    CancelFinishing,
    Screenshot,
    Cut,
    MarkLine,
    /// Picks a file to transcribe in a dialog (FR-18).
    TranscribeFile,
    ContinueRecording(String),
    NewRecording,
    Copy(CopyFormat, Option<String>),
    /// Some words (a line) onto the clipboard.
    CopyText(String),
    Export(ExportKind, Option<String>, bool),
    OpenFolder(Option<String>),
    Rename(String, String),
    Delete(Vec<String>),
    Recover(String),
    DeferTranscription(bool),
    InstallUpdate,
    /// The result of a command that reports nothing but a toast.
    Done(Result<String, AppError>, &'static str),
    /// A recording started (or failed to).
    Started(Result<(), AppError>),
    /// Sessions were deleted: the views showing them go back.
    Deleted(Vec<String>, Result<usize, AppError>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Markdown,
    Pdf,
    Typst,
}

/// Shows a failed command as a banner (with an action when one helps) or a toast.
pub fn report_error(app: &mut App, e: &AppError, fallback: &str) {
    let banner = |code: ErrorCode, message: &str| Notice::new(NoticeLevel::Error, code.as_str(), message);
    match e.code {
        ErrorCode::MicDenied => app.push_banner(banner(e.code, "micDenied").action("openSettings", "open_mic_privacy")),
        ErrorCode::ModelMissing => app.push_banner(banner(e.code, "modelMissing").action("openModels", "open_models")),
        ErrorCode::DiskFull => app.push_banner(banner(e.code, "diskFull")),
        // A second press while a start or stop is under way.
        ErrorCode::Busy => {}
        ErrorCode::Network => app.toast(t("networkError"), Level::Error),
        ErrorCode::ModelChecksum => app.toast(t("checksumError"), Level::Error),
        ErrorCode::AppLoopbackUnsupported => app.toast(crate::i18n::app_capture_unavailable(), Level::Error),
        ErrorCode::FileUnreadable => {
            let text = match e.message.as_str() {
                "noAudio" => t("fileNoAudio"),
                "unsupported" => t("fileUnsupported"),
                detail => tf("fileUnreadable", &[("detail", &detail)]),
            };
            app.toast(text, Level::Error);
        }
        _ => app.toast(tf(fallback, &[("detail", &e.message)]), Level::Error),
    }
}

fn failed(e: AppError, fallback: &'static str) -> Message {
    Message::Failed(e, fallback)
}

pub fn show_notice(app: &mut App, n: Notice) {
    let text = t_json(&n.message, &n.params);
    if n.code == "shot_added" {
        app.flash = Some(app.now);
        let fallback = n.params.get("fallback").and_then(|v| v.as_bool()).unwrap_or(false);
        let text = if fallback { format!("{text}（{}）", t("fallbackCursorMonitor")) } else { text };
        app.toast(text, Level::Info);
        return;
    }
    if n.toast {
        app.toast(text, n.level.into());
    } else {
        app.push_banner(n);
    }
}

/// Runs the action a notice offers.
pub fn run_notice_action(app: &mut App, command: &str) -> Task {
    match command {
        "open_mic_privacy" => {
            let url = if cfg!(target_os = "macos") {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            } else {
                "ms-settings:privacy-microphone"
            };
            let _ = open::that_detached(url);
            Task::none()
        }
        "switch_to_system" => {
            call(app, ops::switch_to_system, |r| r.err().map_or(Message::Noop, |e| failed(e, "failed")))
        }
        "defer_transcription" => run(app, Action::DeferTranscription(true)),
        "open_models" => Task::done(Message::GoSettings(SettingsTab::Models)),
        "open_hotkey_settings" => Task::done(Message::GoSettings(SettingsTab::Hotkeys)),
        "install_update" => run(app, Action::InstallUpdate),
        _ => Task::none(),
    }
}

/// The session the copy and export actions act on: the live one or the last one shown.
pub fn current_session_id(app: &App) -> Option<String> {
    app.rec.session_id.clone().or_else(|| app.live.session_id.clone())
}

fn toast_done(r: Result<String, AppError>, fallback: &'static str) -> Message {
    Message::Action(Action::Done(r, fallback))
}

/// Transcribes an audio or video file into a new session (FR-18): one dropped on the window, or
/// one picked in a dialog. The main view follows it as it finishes, then shows it like any session.
pub fn transcribe_file(app: &mut App, path: Option<PathBuf>) -> Task {
    if app.rec.state != UiState::Ready {
        return Task::none();
    }
    let parent = app.hwnd;
    call(
        app,
        move |c| {
            let path = match path {
                Some(p) => Some(p),
                None => {
                    let ja = crate::i18n::is_ja();
                    let mut dialog = rfd::FileDialog::new()
                        .add_filter(if ja { "音声・動画ファイル" } else { "Audio and video" }, ops::MEDIA_EXTENSIONS)
                        .add_filter(if ja { "すべてのファイル" } else { "All files" }, &["*"]);
                    dialog = with_parent(dialog, parent);
                    dialog.pick_file()
                }
            };
            path.map(|p| ops::transcribe_file(c, p).map(|_| ()))
        },
        |r| match r {
            None => Message::Noop,
            Some(r) => Message::Action(Action::Started(r)),
        },
    )
}

/// Puts a file dialog over the window, so it stays in front of an always-on-top window.
pub fn with_parent(dialog: rfd::FileDialog, hwnd: Option<u64>) -> rfd::FileDialog {
    match hwnd.and_then(Parent::new) {
        Some(parent) => dialog.set_parent(&parent),
        None => dialog,
    }
}

/// The window as a dialog's parent.
pub struct Parent(raw_window_handle::RawWindowHandle);

impl Parent {
    fn new(hwnd: u64) -> Option<Self> {
        #[cfg(windows)]
        {
            let h = std::num::NonZeroIsize::new(hwnd as isize)?;
            Some(Parent(raw_window_handle::RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(h))))
        }
        #[cfg(target_os = "macos")]
        {
            let view = std::ptr::NonNull::new(hwnd as usize as *mut std::ffi::c_void)?;
            Some(Parent(raw_window_handle::RawWindowHandle::AppKit(raw_window_handle::AppKitWindowHandle::new(view))))
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = hwnd;
            None
        }
    }
}

impl raw_window_handle::HasDisplayHandle for Parent {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(raw_window_handle::DisplayHandle::windows())
    }
}

impl raw_window_handle::HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // SAFETY: the handle is the app's own window, which outlives the modal dialog.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(self.0) })
    }
}

/// Asks where to save an export: Downloads first, named after the session folder.
fn ask_save_path(hwnd: Option<u64>, folder: &std::path::Path, extension: &str, kind: &str) -> Option<PathBuf> {
    let stem = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "transcript".into());
    let mut dialog = rfd::FileDialog::new().set_file_name(format!("{stem}.{extension}")).add_filter(kind, &[extension]);
    if let Some(dir) = dirs_download() {
        dialog = dialog.set_directory(dir);
    }
    with_parent(dialog, hwnd).save_file()
}

fn ask_folder(hwnd: Option<u64>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(dir) = dirs_download() {
        dialog = dialog.set_directory(dir);
    }
    with_parent(dialog, hwnd).pick_folder()
}

fn dirs_download() -> Option<PathBuf> {
    // USERPROFILE on Windows, HOME on macOS.
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join("Downloads"))
        .filter(|d| d.is_dir())
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn request_quit(app: &mut App) -> Task {
    if app.rec.busy() {
        // Quitting while recording asks first.
        crate::screens::dialogs::ask_confirm_quit(app);
        return app.on_window_action(crate::app::WinAction::Show);
    }
    crate::task::exit()
}

/// A pick in a menu that `actions` itself built (`action:` ids).
pub fn on_menu(app: &mut App, rest: &str) -> Task {
    let (verb, arg) = rest.split_once(':').unwrap_or((rest, ""));
    let id = (!arg.is_empty()).then(|| arg.to_string());
    match verb {
        "copy-plain" => run(app, Action::Copy(CopyFormat::Plain, id)),
        "copy-agent" => run(app, Action::Copy(CopyFormat::Agent, id)),
        "copy-markdown" => run(app, Action::Copy(CopyFormat::Markdown, id)),
        "export-markdown" => run(app, Action::Export(ExportKind::Markdown, id, false)),
        "export-zip" => run(app, Action::Export(ExportKind::Markdown, id, true)),
        "export-pdf" => run(app, Action::Export(ExportKind::Pdf, id, false)),
        "export-typst" => run(app, Action::Export(ExportKind::Typst, id, false)),
        "continue" => id.map_or(Task::none(), |id| run(app, Action::ContinueRecording(id))),
        "open-folder" => run(app, Action::OpenFolder(id)),
        "recover" => id.map_or(Task::none(), |id| run(app, Action::Recover(id))),
        "delete" => {
            if let Some(id) = id {
                crate::screens::dialogs::ask_delete(app, vec![id], None);
            }
            Task::none()
        }
        "rename" => {
            if let Some(id) = id {
                crate::screens::dialogs::ask_rename(app, id);
            }
            Task::none()
        }
        _ => Task::none(),
    }
}

pub fn run(app: &mut App, action: Action) -> Task {
    match action {
        Action::ToggleRecording => match app.rec.state {
            UiState::Recording | UiState::Paused => {
                call(app, ops::stop_recording, |r| r.err().map_or(Message::Noop, |e| failed(e, "failed")))
            }
            UiState::Ready => {
                let cfg = app.source.config();
                if cfg.mode == SourceMode::App && cfg.app.is_none() {
                    app.toast(t("chooseApp"), Level::Warn);
                    return Task::none();
                }
                call(
                    app,
                    move |c| ops::start_recording(c, cfg, None).map(|_| ()),
                    |r| Message::Action(Action::Started(r)),
                )
            }
            _ => Task::none(),
        },
        Action::Started(Ok(())) => {
            app.go(View::Main);
            app.sync_layout()
        }
        Action::Started(Err(e)) => {
            report_error(app, &e, "startFailed");
            Task::none()
        }
        Action::Stop => call(app, ops::stop_recording, |r| r.err().map_or(Message::Noop, |e| failed(e, "failed"))),
        Action::CancelFinishing => {
            ops::cancel_finishing(&app.core);
            Task::none()
        }
        Action::PauseResume => match app.rec.state {
            UiState::Recording => {
                call(app, ops::pause_recording, |r| r.err().map_or(Message::Noop, |e| failed(e, "failed")))
            }
            UiState::Paused => {
                call(app, ops::resume_recording, |r| r.err().map_or(Message::Noop, |e| failed(e, "failed")))
            }
            _ => Task::none(),
        },
        Action::Screenshot => call(app, ops::take_screenshot, |r| match r {
            // "Not recording" and quick repeats are reported by the core as notices.
            Err(e) if e.code != ErrorCode::Internal => failed(e, "failed"),
            _ => Message::Noop,
        }),
        Action::Cut => call(app, ops::add_cut, |r| toast_done(r.map(|_| t("cutAdded")), "failed")),
        Action::MarkLine => call(app, ops::mark_current_line, |r| {
            toast_done(r.map(|marked| t(if marked { "markAdded" } else { "nothingToMark" })), "failed")
        }),
        Action::TranscribeFile => transcribe_file(app, None),
        Action::ContinueRecording(id) => {
            let cfg = app.source.config();
            if cfg.mode == SourceMode::App && cfg.app.is_none() {
                app.toast(t("chooseApp"), Level::Warn);
                return Task::none();
            }
            // The same session may be on screen already: reloaded for the new part's clock.
            let reload = app.live.session_id.as_deref() == Some(id.as_str());
            call(
                app,
                move |c| {
                    let started = ops::continue_recording(c, id.clone(), cfg).map(|_| ());
                    let view = if started.is_ok() && reload { ops::get_session(c, &id).ok() } else { None };
                    (started, view)
                },
                |(started, view)| match (started, view) {
                    (Ok(()), Some(v)) => Message::LiveLoaded(Box::new(v)),
                    (r, _) => Message::Action(Action::Started(r)),
                },
            )
            .chain(Task::done(Message::Go(View::Main)))
        }
        Action::NewRecording => {
            if app.rec.state == UiState::Ready {
                app.live.clear();
                app.go(View::Main);
            }
            Task::none()
        }
        Action::Copy(format, id) => {
            let Some(id) = id.or_else(|| current_session_id(app)) else { return Task::none() };
            call(app, move |c| ops::copy_transcript(c, &id, format).map(|_| t("copied")), |r| toast_done(r, "failed"))
        }
        Action::CopyText(text) => {
            call(app, move |c| ops::copy_text(c, &text).map(|_| t("copied")), |r| toast_done(r, "failed"))
        }
        Action::Export(kind, id, zip) => {
            let Some(id) = id.or_else(|| current_session_id(app)) else { return Task::none() };
            let hwnd = app.hwnd;
            call(
                app,
                move |c| -> Option<Result<String, AppError>> {
                    let folder = match ops::folder_of(c, &id) {
                        Ok(f) => f,
                        Err(e) => return Some(Err(e)),
                    };
                    let saved = match (kind, zip) {
                        (ExportKind::Markdown, true) => {
                            let out = ask_save_path(hwnd, &folder, "zip", "ZIP")?;
                            ops::export_markdown(c, &id, true, &out)
                        }
                        (ExportKind::Markdown, false) => {
                            let out = ask_folder(hwnd)?;
                            ops::export_markdown(c, &id, false, &out)
                        }
                        (ExportKind::Pdf, _) => {
                            let out = ask_save_path(hwnd, &folder, "pdf", "PDF")?;
                            match ops::export_pdf(c, &id, &out) {
                                // The PDF opens; the fallback HTML opens in the browser to print.
                                Ok(PdfExport::Pdf(p)) => {
                                    let _ = open::that_detached(&p);
                                    Ok(p)
                                }
                                Ok(PdfExport::Html(p)) => {
                                    let _ = open::that_detached(&p);
                                    return None;
                                }
                                Err(e) => Err(e),
                            }
                        }
                        (ExportKind::Typst, _) => {
                            let out = ask_save_path(hwnd, &folder, "typ", "Typst")?;
                            ops::export_typst(c, &id, &out)
                        }
                    };
                    Some(saved.map(|p| tf("savedAs", &[("name", &file_name(&p))])))
                },
                // A cancelled dialog says nothing.
                |r| r.map_or(Message::Noop, |r| toast_done(r, "exportFailed")),
            )
        }
        Action::OpenFolder(id) => {
            let folder = match id {
                Some(id) => ops::folder_of(&app.core, &id).ok(),
                None => app
                    .rec
                    .folder
                    .clone()
                    .map(PathBuf::from)
                    .or_else(|| app.live.folder.clone())
                    .or_else(|| app.rec.last_saved_folder.clone().map(PathBuf::from)),
            };
            if let Some(folder) = folder
                && ops::allowed_path(&app.core, &folder)
            {
                let _ = open::that_detached(folder);
            }
            Task::none()
        }
        Action::Rename(id, title) => {
            if app.live.session_id.as_deref() == Some(id.as_str()) {
                app.live.title = Some(title.trim().to_string());
            }
            call(app, move |c| ops::rename_session(c, &id, &title).map(|_| t("saved")), |r| toast_done(r, "saveFailed"))
        }
        Action::Delete(ids) => {
            let ids2 = ids.clone();
            call(
                app,
                move |c| {
                    if ids2.len() == 1 {
                        ops::delete_session(c, &ids2[0]).map(|_| 1)
                    } else {
                        ops::delete_sessions(c, &ids2)
                    }
                },
                move |r| Message::Action(Action::Deleted(ids, r)),
            )
        }
        Action::Deleted(ids, r) => {
            match r {
                Ok(n) if ids.len() == 1 => {
                    let _ = n;
                    app.toast(t("deleted"), Level::Info);
                }
                Ok(n) => app.toast(tf("deletedMany", &[("n", &n)]), Level::Info),
                Err(e) => report_error(app, &e, "deleteFailed"),
            }
            // A deleted session can't stay on screen.
            if app.live.session_id.as_ref().is_some_and(|s| ids.contains(s)) && app.rec.state == UiState::Ready {
                app.live.clear();
            }
            if app.view == View::Session && app.viewing_session.as_ref().is_some_and(|s| ids.contains(s)) {
                app.go(View::History);
            }
            crate::screens::history::on_open(app)
        }
        Action::Recover(id) => {
            let done = call(
                app,
                move |c| ops::recover_session(c, &id).map(|_| t("recovered")),
                |r| toast_done(r, "saveFailed"),
            );
            // The row loses its 要復元 tag once the list reloads.
            done.chain(crate::screens::history::reload(app))
        }
        Action::DeferTranscription(deferred) => call(
            app,
            move |c| ops::defer_transcription(c, deferred),
            |r| r.err().map_or(Message::Noop, |e| failed(e, "failed")),
        ),
        Action::InstallUpdate => {
            app.toast(t("updateDownloading"), Level::Info);
            call(app, crate::updater::install, |r| match r {
                Ok(()) => Message::Noop,
                Err(e) if e.message == "updateWhileRecording" => Message::Toast(t("updateWhileRecording"), Level::Warn),
                Err(e) if e.code == ErrorCode::Network => Message::Toast(t("networkError"), Level::Error),
                Err(e) => Message::Toast(tf("updateFailed", &[("detail", &e.message)]), Level::Error),
            })
        }
        Action::Done(Ok(text), _) => {
            app.toast(text, Level::Info);
            Task::none()
        }
        Action::Done(Err(e), fallback) => {
            report_error(app, &e, fallback);
            Task::none()
        }
    }
}
