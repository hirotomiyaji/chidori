//! What to listen to: the source picker's choice (FR-10, FR-11, FR-12), and how loud each app it
//! shows is (the waves beside their names).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::paint::Picture;
use chidori_core::audio::source::{AppLevel, AudioApp, Device};
use chidori_core::recorder::{AppRef, SourceConfig};
use chidori_core::settings::{Settings, SourceMode};

use super::line::{height_of, settle};

#[derive(Debug, Clone, PartialEq)]
pub struct ChosenApp {
    pub exe: String,
    pub name: String,
    /// 0 for the last-used app until the list finds it running.
    pub root_pid: u32,
}

#[derive(Debug, Default)]
pub struct SourceChoice {
    pub mode: SourceMode,
    pub app: Option<ChosenApp>,
    pub include_mic: bool,
    pub mic_device_id: Option<String>,
    pub apps: Vec<AudioApp>,
    /// Each app's icon, decoded once from its data URL.
    pub icons: HashMap<String, Picture>,
    pub devices: Vec<Device>,
    pub loading_apps: bool,
}

impl SourceChoice {
    pub fn init_from(&mut self, s: &Settings) {
        self.mode = s.source.mode;
        self.include_mic = s.source.include_mic;
        self.mic_device_id = s.source.mic_device_id.clone();
        self.app = s.source.app.as_ref().map(|a| ChosenApp { exe: a.exe.clone(), name: a.name.clone(), root_pid: 0 });
    }

    pub fn set_app(&mut self, a: &AudioApp) {
        self.app = Some(ChosenApp { exe: a.exe.clone(), name: a.name.clone(), root_pid: a.root_pid });
    }

    /// A fresh list of apps; the chosen app is bound again to its current process.
    pub fn set_apps(&mut self, apps: Vec<AudioApp>) {
        if let Some(current) = &mut self.app
            && let Some(m) = apps.iter().find(|a| a.exe.eq_ignore_ascii_case(&current.exe))
        {
            *current = ChosenApp { exe: m.exe.clone(), name: m.name.clone(), root_pid: m.root_pid };
        }
        for a in &apps {
            if !self.icons.contains_key(&a.exe)
                && let Some(handle) = a.icon_data_url.as_deref().and_then(icon_from_data_url)
            {
                self.icons.insert(a.exe.clone(), handle);
            }
        }
        self.apps = apps;
        self.loading_apps = false;
    }

    pub fn icon_of(&self, exe: &str) -> Option<&Picture> {
        self.icons.get(exe)
    }

    pub fn config(&self) -> SourceConfig {
        SourceConfig {
            mode: self.mode,
            app: match (self.mode, &self.app) {
                (SourceMode::App, Some(a)) => {
                    Some(AppRef { exe: a.exe.clone(), root_pid: a.root_pid, name: Some(a.name.clone()) })
                }
                _ => None,
            },
            include_mic: self.mode != SourceMode::Mic && self.include_mic,
            mic_device_id: self.mic_device_id.clone(),
        }
    }

    /// The app is picked by its process when known, else by its executable.
    pub fn is_chosen(&self, a: &AudioApp) -> bool {
        self.mode == SourceMode::App
            && self.app.as_ref().is_some_and(|c| {
                if c.root_pid != 0 { c.root_pid == a.root_pid } else { c.exe.eq_ignore_ascii_case(&a.exe) }
            })
    }

    pub fn mic_name(&self) -> Option<String> {
        match &self.mic_device_id {
            Some(id) => self.devices.iter().find(|d| &d.id == id).map(|d| d.name.clone()),
            None => self.devices.iter().find(|d| d.is_default).map(|d| d.name.clone()),
        }
    }
}

fn icon_from_data_url(url: &str) -> Option<Picture> {
    use base64::Engine as _;
    let (_, b64) = url.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let img = ::image::load_from_memory(&bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Picture::from_rgba(w, h, img.into_raw())
}

/// One step of a wave: ten a second, like the recording's line.
pub const STEP: Duration = Duration::from_millis(100);
/// Steps across a wave: two seconds.
pub const WAVE_STEPS: usize = 20;
/// Reports come ten times a second while there is sound and stop in silence; this late, it has stopped.
const STALE: Duration = Duration::from_millis(400);
/// A height this low is flat.
const FLAT: f32 = 0.02;

#[derive(Debug, Clone)]
pub struct Track {
    /// Heights 0..1, one per step, newest last.
    pub heights: Vec<f32>,
    at: Instant,
    level: f32,
    heard_at: Option<Instant>,
}

impl Track {
    fn new(now: Instant) -> Self {
        Self { heights: vec![0.0; WAVE_STEPS + 1], at: now, level: 0.0, heard_at: None }
    }

    /// Brings the track up to `now`: a step every 100 ms, at the reported height while reports
    /// come, else flat.
    fn advance(&mut self, now: Instant) {
        if now.saturating_duration_since(self.at) > STEP * (WAVE_STEPS as u32 + 1) {
            self.heights.iter_mut().for_each(|h| *h = 0.0);
            self.at = now;
        }
        while now.saturating_duration_since(self.at) >= STEP {
            self.at += STEP;
            let fresh = self.heard_at.is_some_and(|h| self.at.saturating_duration_since(h) <= STALE);
            let level = if fresh { self.level } else { 0.0 };
            let last = *self.heights.last().unwrap_or(&0.0);
            self.heights.push(settle(last, level));
            self.heights.remove(0);
        }
    }

    /// Nothing to show: flat all along and nothing heard lately.
    pub fn quiet(&self, now: Instant) -> bool {
        let hearing = self.heard_at.is_some_and(|h| now.saturating_duration_since(h) <= STALE) && self.level > FLAT;
        !hearing && self.heights.iter().all(|h| *h <= FLAT)
    }
}

/// The waves of the apps on screen, by root PID.
#[derive(Debug, Default)]
pub struct AppLevels {
    pub tracks: HashMap<u32, Track>,
    /// The PIDs last asked of the core, so it is told only when they change.
    pub watched: Vec<u32>,
}

impl AppLevels {
    /// The apps to watch; returns the new list when it changed (to send to the core).
    pub fn watch(&mut self, mut pids: Vec<u32>, now: Instant) -> Option<Vec<u32>> {
        pids.sort_unstable();
        pids.dedup();
        if pids == self.watched {
            return None;
        }
        self.tracks.retain(|pid, _| pids.contains(pid));
        for pid in &pids {
            self.tracks.entry(*pid).or_insert_with(|| Track::new(now));
        }
        self.watched = pids.clone();
        Some(pids)
    }

    pub fn hear(&mut self, levels: &[AppLevel], now: Instant) {
        for l in levels {
            if let Some(t) = self.tracks.get_mut(&l.root_pid) {
                t.level = height_of(Some(l.dbfs));
                t.heard_at = Some(now);
            }
        }
    }

    pub fn advance(&mut self, now: Instant) {
        self.tracks.values_mut().for_each(|t| t.advance(now));
    }

    /// Whether any wave moves, so the app keeps ticking.
    pub fn moving(&self, now: Instant) -> bool {
        self.tracks.values().any(|t| !t.quiet(now))
    }
}
