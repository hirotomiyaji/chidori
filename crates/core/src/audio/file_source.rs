//! An audio or video file as a source (FR-18), and the integration harness's WAV player
//! (section 18). The file's own clock places its sound: session time is time in the file, however
//! fast it is read.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use parking_lot::Mutex;

use super::decode;
use super::source::{AudioSource, RawChunk};
use crate::session::model::SourceId;

/// How fast the file is read.
#[derive(Clone)]
pub enum Pace {
    /// 1.0 = real time; larger plays faster (the harness's latency figures need real time).
    Speed(f32),
    /// As fast as transcription takes it: reading waits while `behind` says enough is queued, so a
    /// long file never sits in memory as waiting speech.
    AheadOf(Arc<dyn Fn() -> bool + Send + Sync>),
}

/// How far the reading has got, for the progress the UI shows.
#[derive(Debug, Default)]
pub struct FileProgress {
    /// The file's length as its container states it; 0 when unknown.
    pub duration_ms: AtomicU64,
    /// Read and handed to the DSP so far.
    pub read_ms: AtomicU64,
    /// The whole file was read, or reading stopped on an error.
    pub done: AtomicBool,
    /// Why reading stopped before the end.
    pub error: Mutex<Option<String>>,
}

pub struct FileSource {
    id: SourceId,
    path: PathBuf,
    /// Use only this channel of a multi-channel file (left = 0), or all channels.
    channel: Option<usize>,
    t0_100ns: u64,
    pace: Pace,
    progress: Arc<FileProgress>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FileSource {
    pub fn new(id: SourceId, path: PathBuf, channel: Option<usize>, t0_100ns: u64, speed: f32) -> Self {
        Self {
            id,
            path,
            channel,
            t0_100ns,
            pace: Pace::Speed(speed),
            progress: Arc::default(),
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }

    pub fn with_pace(mut self, pace: Pace) -> Self {
        self.pace = pace;
        self
    }

    pub fn progress(&self) -> Arc<FileProgress> {
        self.progress.clone()
    }

    /// Waits until the whole file has been sent.
    pub fn join(&mut self) {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Reads a WAV into interleaved f32 samples.
pub fn read_wav(path: &std::path::Path) -> anyhow::Result<(Vec<f32>, u32, u16)> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<_, _>>()?
        }
    };
    Ok((samples, spec.sample_rate, spec.channels))
}

/// Where a stream of blocks is in time, kept in frames so a long file doesn't drift by rounding.
#[derive(Default)]
struct Position {
    /// Time where the current format started, in 100 ns units.
    base_100ns: u64,
    rate: u32,
    frames: u64,
}

impl Position {
    /// The time of the next frame at `rate`, which may be a new one.
    fn at(&mut self, rate: u32) -> u64 {
        if rate != self.rate {
            self.base_100ns = self.now();
            self.rate = rate;
            self.frames = 0;
        }
        self.now()
    }

    fn now(&self) -> u64 {
        if self.rate == 0 { self.base_100ns } else { self.base_100ns + self.frames * 10_000_000 / u64::from(self.rate) }
    }
}

impl AudioSource for FileSource {
    fn id(&self) -> SourceId {
        self.id
    }

    /// Returns once the file is open, or with why it can't be (`decode::DecodeError`).
    fn start(&mut self, tx: Sender<RawChunk>) -> anyhow::Result<()> {
        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<anyhow::Result<()>>(1);
        let (id, path, channel, t0) = (self.id, self.path.clone(), self.channel, self.t0_100ns);
        let (pace, progress, stop) = (self.pace.clone(), self.progress.clone(), self.stop.clone());
        let thread = std::thread::Builder::new().name(format!("file-{}", id.as_str())).spawn(move || {
            // Opened here: Media Foundation's reader stays on its thread.
            let mut decoder = match decode::open(&path) {
                Ok(d) => d,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            progress.duration_ms.store(decoder.duration_ms().unwrap_or(0), Ordering::SeqCst);
            let _ = ready_tx.send(Ok(()));
            let started = Instant::now();
            let mut pos = Position::default();
            while !stop.load(Ordering::SeqCst) {
                let block = match decoder.read() {
                    Ok(Some(b)) => b,
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!("{}: {e:#}", path.display());
                        *progress.error.lock() = Some(format!("{e:#}"));
                        break;
                    }
                };
                let ch = usize::from(block.channels.max(1));
                let (samples, channels) = match channel {
                    Some(c) if c < ch => (block.samples.chunks_exact(ch).map(|f| f[c]).collect(), 1u16),
                    _ => (block.samples, block.channels),
                };
                let at = pos.at(block.rate);
                match &pace {
                    Pace::Speed(speed) => {
                        let due = Duration::from_nanos(at * 100).div_f32(speed.max(0.01));
                        if let Some(wait) = due.checked_sub(started.elapsed()) {
                            std::thread::sleep(wait);
                        }
                    }
                    Pace::AheadOf(behind) => {
                        while behind() && !stop.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                }
                pos.frames += (samples.len() / usize::from(channels.max(1))) as u64;
                let raw = RawChunk {
                    source: id,
                    qpc_100ns: t0 + at,
                    rate: block.rate,
                    channels,
                    samples,
                    silent_flag: false,
                };
                if tx.send(raw).is_err() {
                    break;
                }
                progress.read_ms.store(pos.now() / 10_000, Ordering::SeqCst);
            }
            progress.done.store(true, Ordering::SeqCst);
        })?;
        self.thread = Some(thread);
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                self.join();
                Err(e)
            }
            Err(_) => {
                self.join();
                Err(anyhow::anyhow!("the file reader stopped"))
            }
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::decode::tests::tone_wav;

    #[test]
    fn a_file_is_placed_on_its_own_clock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        tone_wav(&path, 48_000, 2, 3.0);
        let t0 = 1_000_000_000;
        let mut src = FileSource::new(SourceId::File, path, None, t0, 1.0).with_pace(Pace::AheadOf(Arc::new(|| false)));
        let progress = src.progress();
        let (tx, rx) = crossbeam_channel::unbounded();
        src.start(tx).unwrap();
        src.join();
        // Each piece starts where the one before it ended.
        let mut frames = 0u64;
        for c in rx.try_iter() {
            assert_eq!((c.rate, c.channels), (48_000, 2));
            assert_eq!(c.qpc_100ns, t0 + frames * 10_000_000 / 48_000);
            frames += (c.samples.len() / 2) as u64;
        }
        assert!(frames.abs_diff(144_000) <= 2_048, "{frames}");
        assert!(progress.done.load(Ordering::SeqCst));
        assert!(progress.read_ms.load(Ordering::SeqCst).abs_diff(3_000) <= 50);
        assert!(progress.duration_ms.load(Ordering::SeqCst).abs_diff(3_000) <= 50);
    }

    #[test]
    fn a_file_is_read_as_fast_as_it_is_taken() {
        // A minute of sound reads in well under a second, but CI runs the suite in parallel on a
        // shared machine, so the bound is half the length: only pacing to real time can cross it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("minute.wav");
        tone_wav(&path, 8_000, 1, 60.0);
        let mut src = FileSource::new(SourceId::File, path, None, 0, 1.0).with_pace(Pace::AheadOf(Arc::new(|| false)));
        let (tx, _rx) = crossbeam_channel::unbounded();
        let started = Instant::now();
        src.start(tx).unwrap();
        src.join();
        assert!(started.elapsed() < Duration::from_secs(30), "{:?}", started.elapsed());
    }

    #[test]
    fn a_file_that_cannot_be_opened_fails_to_start() {
        let dir = tempfile::tempdir().unwrap();
        let mut src = FileSource::new(SourceId::File, dir.path().join("missing.mp3"), None, 0, 1.0);
        let (tx, _rx) = crossbeam_channel::unbounded();
        assert!(src.start(tx).is_err());
    }

    #[test]
    fn a_new_rate_carries_on_from_the_time_reached() {
        let mut pos = Position::default();
        assert_eq!(pos.at(48_000), 0);
        pos.frames += 48_000;
        // One second in, the stream turns to 44.1 kHz.
        assert_eq!(pos.at(44_100), 10_000_000);
        pos.frames += 22_050;
        assert_eq!(pos.now(), 15_000_000);
    }
}
