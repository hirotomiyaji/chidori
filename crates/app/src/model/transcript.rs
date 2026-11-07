//! A session's transcript as the UI holds it: the live (or last recorded) session, which the
//! core's events feed, or a past one opened from 履歴.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::paint::Picture;
use chidori_core::events::{MarkerPayload, PartialPayload, ScreenshotPayload};
use chidori_core::ops::SessionView;
use chidori_core::session::model::{
    AudioFile, Continuation, Marker, Screenshot, Segment, SourceId, SourceInfo, TimelineItem,
};
use chidori_core::session::recovery::Sound;

#[derive(Debug, Clone)]
pub struct PartialLine {
    pub utterance_id: String,
    pub text: String,
    pub t_start_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Transcript {
    pub session_id: Option<String>,
    pub title: Option<String>,
    pub started_at: Option<String>,
    pub folder: Option<PathBuf>,
    pub duration_ms: Option<u64>,
    pub model_id: Option<String>,
    pub gpu: Option<bool>,
    /// The project's ID (FR-64).
    pub project: Option<String>,
    /// Later recordings onto the session (FR-08), for its clock times.
    pub continued: Vec<Continuation>,
    /// Each recording's sound (FR-09).
    pub audio: Vec<AudioFile>,
    pub sources: Vec<SourceInfo>,
    /// How the finished session sounded, for its line.
    pub sound: Option<Sound>,
    pub items: Vec<TimelineItem>,
    pub partials: HashMap<SourceId, PartialLine>,
    /// Screenshot id → thumbnail (live captures arrive with one).
    pub thumbs: HashMap<String, Picture>,
    /// Items that arrived live a moment ago; they settle in. Each is forgotten after 1.5 s.
    pub fresh: HashMap<String, Instant>,
    /// Counts changes to the items, so lazily built rows rebuild only when something changed.
    pub version: u64,
}

const FRESH_FOR: Duration = Duration::from_millis(1500);

impl Transcript {
    pub fn load(&mut self, view: SessionView) {
        let same = self.session_id.as_deref() == Some(view.session.id.as_str());
        let s = view.session;
        // Keep live items that may have arrived while loading.
        let mut items = s.items;
        if same {
            let ids: std::collections::HashSet<String> = items.iter().map(|i| i.id().to_string()).collect();
            items.extend(self.items.drain(..).filter(|i| !ids.contains(i.id())));
        }
        *self = Transcript {
            session_id: Some(s.id),
            title: Some(s.title),
            started_at: Some(s.started_at),
            folder: Some(view.folder),
            duration_ms: Some(s.duration_ms),
            model_id: Some(s.model.id),
            gpu: Some(s.gpu),
            project: s.project,
            continued: s.continued,
            audio: s.audio,
            sources: s.sources,
            sound: view.sound,
            items,
            partials: if same { std::mem::take(&mut self.partials) } else { HashMap::new() },
            thumbs: if same { std::mem::take(&mut self.thumbs) } else { HashMap::new() },
            fresh: if same { std::mem::take(&mut self.fresh) } else { HashMap::new() },
            version: self.version + 1,
        };
    }

    pub fn clear(&mut self) {
        *self = Transcript { version: self.version + 1, ..Default::default() };
    }

    fn arrived(&mut self, id: &str) {
        self.fresh.insert(id.to_string(), Instant::now());
        self.version += 1;
    }

    /// Forgets items that are no longer fresh; true when something changed.
    pub fn age(&mut self, now: Instant) -> bool {
        let before = self.fresh.len();
        self.fresh.retain(|_, at| now.duration_since(*at) < FRESH_FOR);
        let changed = self.fresh.len() != before;
        if changed {
            self.version += 1;
        }
        changed
    }

    pub fn has(&self, id: &str) -> bool {
        self.items.iter().any(|i| i.id() == id)
    }

    pub fn add_segment(&mut self, seg: Segment) {
        if self.has(&seg.id) {
            return;
        }
        if let (Some(p), Some(u)) = (self.partials.get(&seg.source), &seg.utterance_id)
            && &p.utterance_id == u
        {
            self.partials.remove(&seg.source);
        }
        let id = seg.id.clone();
        self.items.push(TimelineItem::Segment(seg));
        self.arrived(&id);
    }

    pub fn remove_item(&mut self, id: &str) {
        self.items.retain(|i| i.id() != id);
        self.version += 1;
    }

    pub fn update_segment(&mut self, seg: Segment) {
        for i in &mut self.items {
            if let TimelineItem::Segment(s) = i
                && s.id == seg.id
            {
                *s = seg;
                self.version += 1;
                return;
            }
        }
    }

    pub fn add_screenshot(&mut self, p: ScreenshotPayload) {
        if self.has(&p.id) {
            return;
        }
        if let Some(handle) = thumb_from_data_url(&p.thumb_data_url) {
            self.thumbs.insert(p.id.clone(), handle);
        }
        let id = p.id.clone();
        self.items.push(TimelineItem::Screenshot(Screenshot {
            id: p.id,
            t_ms: p.t_ms,
            file: p.file,
            width: p.width,
            height: p.height,
            caption: None,
        }));
        self.arrived(&id);
    }

    pub fn set_caption(&mut self, id: &str, caption: &str) {
        for i in &mut self.items {
            if let TimelineItem::Screenshot(s) = i
                && s.id == id
            {
                s.caption = (!caption.is_empty()).then(|| caption.to_string());
                self.version += 1;
            }
        }
    }

    pub fn add_marker(&mut self, p: MarkerPayload) {
        if self.has(&p.id) {
            return;
        }
        let id = p.id.clone();
        self.items.push(TimelineItem::Marker(Marker { id: p.id, t_ms: p.t_ms, kind: p.kind, detail: p.detail }));
        self.arrived(&id);
    }

    pub fn set_partial(&mut self, p: PartialPayload) {
        // A partial for an utterance that already has its final line is stale.
        let finalized = self.items.iter().any(
            |i| matches!(i, TimelineItem::Segment(s) if s.utterance_id.as_deref() == Some(p.utterance_id.as_str())),
        );
        if p.text.is_empty() || finalized {
            if self.partials.get(&p.source).is_some_and(|x| x.utterance_id == p.utterance_id) {
                self.partials.remove(&p.source);
            }
        } else {
            self.partials
                .insert(p.source, PartialLine { utterance_id: p.utterance_id, text: p.text, t_start_ms: p.t_start_ms });
        }
    }

    /// Both 相手 and 自分 were recorded: lines carry speaker names.
    pub fn two_sides(&self) -> bool {
        let mic = self.sources.iter().any(|s| s.id == SourceId::Mic);
        let other = self.sources.iter().any(|s| s.id != SourceId::Mic);
        mic && other
    }

    pub fn is_from_file(&self) -> bool {
        self.sources.iter().any(|s| s.id == SourceId::File)
    }

    /// The clock time of a moment in the session.
    pub fn clock(&self, t_ms: u64) -> String {
        match &self.started_at {
            Some(start) => super::format::session_clock(start, &self.continued, self.is_from_file(), t_ms),
            None => super::format::hms(t_ms),
        }
    }
}

/// A live screenshot's thumbnail, sent as a `data:image/...;base64,` URL.
fn thumb_from_data_url(url: &str) -> Option<Picture> {
    use base64::Engine as _;
    let (_, b64) = url.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let img = ::image::load_from_memory(&bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Picture::from_rgba(w, h, img.into_raw())
}
