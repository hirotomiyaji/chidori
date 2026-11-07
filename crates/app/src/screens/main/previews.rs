//! 録音 in the states a recording passes through, for tests of what it shows.

use std::time::{Duration, Instant};

use chidori_core::events::{PartialPayload, RecordedSources, UiState};
use chidori_core::session::model::{Marker, MarkerKind, Screenshot, Segment, SourceId, SourceInfo, TimelineItem};
use chidori_core::settings::SourceMode;

use crate::app::App;
use crate::model::line::Sample;
use crate::model::source::ChosenApp;
use crate::preview::app;

fn seg(id: &str, source: SourceId, t: u64, text: &str, important: bool) -> TimelineItem {
    TimelineItem::Segment(Segment {
        id: id.into(),
        source,
        t_start_ms: t,
        t_end_ms: t + 6000,
        text: text.into(),
        text_original: None,
        edited: false,
        utterance_id: None,
        important,
    })
}

const START: u64 = 4_480_000;

fn lines() -> Vec<TimelineItem> {
    vec![
        seg("s1", SourceId::App, START, "リリースの日程ですが、11月の第2週で変わりありません。", false),
        seg(
            "s2",
            SourceId::Mic,
            START + 9_000,
            "了解です。こちらはインストーラーの署名が終わったので、あとはテストだけです。",
            false,
        ),
        TimelineItem::Marker(Marker { id: "m1".into(), t_ms: START + 15_000, kind: MarkerKind::Cut, detail: None }),
        seg("s3", SourceId::App, START + 18_000, "ありがとうございます。テストはいつごろ終わりそうですか。", true),
        seg("s4", SourceId::Mic, START + 27_000, "金曜までには終わらせます。", false),
        TimelineItem::Screenshot(Screenshot {
            id: "img1".into(),
            t_ms: START + 31_000,
            file: "images/img1.png".into(),
            width: 1600,
            height: 900,
            caption: Some("スケジュール表".into()),
        }),
        seg(
            "s5",
            SourceId::App,
            START + 36_000,
            "では金曜にもう一度集まりましょう。資料はこちらで用意しておきます。",
            false,
        ),
    ]
}

/// The line: alternating turns of 相手 and 自分, with a screenshot.
fn samples() -> Vec<Sample> {
    (0..400)
        .map(|i| {
            let f = i as f32;
            let turn = (i / 60) % 2 == 0;
            let level = 0.35 + 0.25 * (f * 0.7).sin() * (f * 0.13).cos();
            Sample {
                o: if turn { level.max(0.0) } else { 0.02 },
                m: if turn { 0.0 } else { (level * 0.9).max(0.0) },
                shot: i == 310,
                gap: false,
            }
        })
        .collect()
}

fn session(app: &mut App, id: &str) {
    let t = &mut app.live;
    t.session_id = Some(id.into());
    t.title = Some("週次ミーティング".into());
    t.started_at = Some("2026-10-02T14:02:00+09:00".into());
    t.sources = vec![
        SourceInfo {
            id: SourceId::App,
            label: "Zoom".into(),
            exe: Some("Zoom.exe".into()),
            device: None,
            name: Some("Zoom".into()),
        },
        SourceInfo { id: SourceId::Mic, label: "自分".into(), exe: None, device: None, name: None },
    ];
    t.items = lines();
    t.version += 1;
    app.source.mode = SourceMode::App;
    app.source.app = Some(ChosenApp { exe: "Zoom.exe".into(), name: "Zoom".into(), root_pid: 0 });
    app.source.include_mic = true;
}

fn recording(app: &mut App) {
    session(app, "s1");
    app.rec.state = UiState::Recording;
    app.rec.session_id = Some("s1".into());
    app.rec.sources = Some(RecordedSources { ids: vec![SourceId::App, SourceId::Mic], app_name: Some("Zoom".into()) });
    let now = Instant::now();
    app.now = now;
    app.rec.elapsed_ms = 757_000;
    app.rec.elapsed_at = now;
    app.line.session_id = Some("s1".into());
    app.line.samples = samples();
    app.line.at = Some(now - Duration::from_millis(40));
    app.live.set_partial(PartialPayload {
        source: SourceId::Mic,
        utterance_id: "u9".into(),
        text: "わかりました、それで".into(),
        t_start_ms: Some(START + 44_000),
    });
}

#[test]
fn the_camera_menu_checks_what_screenshots_take() {
    crate::i18n::set_locale(chidori_core::settings::Locale::Ja);
    let (mut app, _dir) = app();
    recording(&mut app);
    let w = |id: u32, title: &str| chidori_core::screenshot::WindowInfo { id, title: title.into(), app: "App".into() };
    let items = super::deck::shot_menu(&app, vec![w(1, "Q&A"), w(2, &"長".repeat(80))]);
    let checks: Vec<(String, bool)> = items
        .iter()
        .filter_map(|i| match i {
            crate::native::menu::Item::Check { label, checked, .. } => Some((label.clone(), *checked)),
            _ => None,
        })
        .collect();
    assert_eq!(checks[0], ("Zoomのウィンドウ".to_string(), true));
    assert!(!checks[1].1 && !checks[2].1);
    let Some(crate::native::menu::Item::Submenu { items: windows, .. }) = items.last() else { panic!("windows") };
    let labels: Vec<String> = windows
        .iter()
        .filter_map(|i| match i {
            crate::native::menu::Item::Check { label, .. } => Some(label.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(labels[0], "Q&&A");
    assert_eq!(labels[1].chars().count(), 60);
    // A picked window holds for the recording; the targets lose their check.
    app.rec.shot_window = Some(w(9, "Slides"));
    let items = super::deck::shot_menu(&app, Vec::new());
    assert!(items.iter().all(|i| !matches!(i, crate::native::menu::Item::Check { checked: true, .. })));
}

#[test]
fn speech_waiting_for_text_is_shown_as_being_written() {
    let (mut app, _dir) = app();
    recording(&mut app);
    let lag = |ms| chidori_core::events::LagPayload { lag_ms: ms, queued: 1, device: "cpu" };
    app.rec.lag = Some(lag(800));
    assert!(!super::transcript::writing(&app));
    app.rec.lag = Some(lag(4000));
    assert!(super::transcript::writing(&app));
    // Waiting for Stop on purpose is not being written.
    app.rec.deferred = true;
    assert!(!super::transcript::writing(&app));
}

#[test]
fn the_phase_follows_the_recording_and_the_session_shown() {
    let (mut app, _dir) = app();
    assert_eq!(super::phase(&app), super::Phase::Start);
    recording(&mut app);
    assert_eq!(super::phase(&app), super::Phase::Live);
    app.rec.state = UiState::Ready;
    assert_eq!(super::phase(&app), super::Phase::Review);
}
