//! Core Audio capture for the mic and the whole system's sound (section 7, as on Windows).
//!
//! One thread per source owns the `cpal` stream. Core Audio calls back on its own real-time
//! thread with each buffer's host time, which `platform::qpc_now_100ns` shares, so buffers are
//! placed by timestamp, never by counting samples. The system source is a loopback stream on the
//! default output, which `cpal` builds from a Core Audio process tap (macOS 14.6+).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Sender, TrySendError, bounded};

use super::devices;
use crate::audio::source::{AudioSource, CaptureError, CaptureTarget, RawChunk, SourceStatus};
use crate::session::model::SourceId;

const NO_PACKETS_RESTART: Duration = Duration::from_secs(5);
const DEFAULT_CHANGE_SETTLE: Duration = Duration::from_millis(1500);

pub struct CoreAudioSource {
    id: SourceId,
    target: CaptureTarget,
    status: Sender<SourceStatus>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CoreAudioSource {
    pub fn new(id: SourceId, target: CaptureTarget, status: Sender<SourceStatus>) -> Self {
        Self { id, target, status, stop: Arc::new(AtomicBool::new(false)), thread: None }
    }
}

impl AudioSource for CoreAudioSource {
    fn id(&self) -> SourceId {
        self.id
    }

    fn start(&mut self, tx: Sender<RawChunk>) -> anyhow::Result<()> {
        if matches!(self.target, CaptureTarget::App { .. }) {
            return Err(CaptureError::Other("app capture is not available on macOS yet".into()).into());
        }
        let (init_tx, init_rx) = bounded::<Result<(), CaptureError>>(1);
        let ctx = ThreadCtx {
            id: self.id,
            target: self.target.clone(),
            status: self.status.clone(),
            stop: self.stop.clone(),
            tx,
        };
        let handle = std::thread::Builder::new()
            .name(format!("capture-{}", self.id.as_str()))
            .spawn(move || capture_thread(ctx, init_tx))?;
        match init_rx.recv_timeout(Duration::from_secs(8)) {
            Ok(Ok(())) => {
                self.thread = Some(handle);
                Ok(())
            }
            Ok(Err(err)) => {
                let _ = handle.join();
                Err(err.into())
            }
            Err(_) => {
                self.stop.store(true, Ordering::SeqCst);
                Err(anyhow::anyhow!("timed out opening the {} stream", self.id.as_str()))
            }
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for CoreAudioSource {
    fn drop(&mut self) {
        self.stop();
    }
}

struct ThreadCtx {
    id: SourceId,
    target: CaptureTarget,
    status: Sender<SourceStatus>,
    stop: Arc<AtomicBool>,
    tx: Sender<RawChunk>,
}

/// What the real-time callback tells the capture thread.
#[derive(Default)]
struct Shared {
    /// Host time of the last buffer, in 100 ns units (0: none yet).
    last_packet: AtomicU64,
    /// The device went away or the stream broke; reopen.
    broken: AtomicBool,
    /// The DSP side hung up.
    closed: AtomicBool,
    dropped: AtomicU64,
}

struct Stream {
    _stream: cpal::Stream,
    rate: u32,
    channels: u16,
    device: Option<String>,
    device_id: Option<String>,
}

fn classify(err: cpal::Error) -> CaptureError {
    match err.kind() {
        cpal::ErrorKind::PermissionDenied => CaptureError::MicDenied,
        _ => CaptureError::Other(err.to_string()),
    }
}

fn open(ctx: &ThreadCtx, shared: &Arc<Shared>) -> Result<Stream, CaptureError> {
    let (device, config) = match &ctx.target {
        CaptureTarget::Mic { device_id } => {
            let device = devices::mic_device(device_id.as_deref())
                .ok_or_else(|| CaptureError::Other("no microphone found".into()))?;
            let config = device.default_input_config().map_err(classify)?;
            (device, config)
        }
        CaptureTarget::System => {
            let device = cpal::default_host()
                .default_output_device()
                .ok_or_else(|| CaptureError::Other("no output device found".into()))?;
            // An input stream on an output device is loopback: its format is the output's.
            let config = device.default_output_config().map_err(classify)?;
            (device, config)
        }
        CaptureTarget::App { .. } => unreachable!("refused in start"),
    };
    let rate = config.sample_rate();
    let channels = config.channels();
    let stream_config = cpal::StreamConfig { channels, sample_rate: rate, buffer_size: cpal::BufferSize::Default };

    let id = ctx.id;
    let tx = ctx.tx.clone();
    let data_shared = shared.clone();
    let err_shared = shared.clone();
    let stream = device
        .build_input_stream::<f32, _, _>(
            stream_config,
            move |data: &[f32], info: &cpal::InputCallbackInfo| {
                if data.is_empty() {
                    return;
                }
                let qpc = (info.timestamp().capture.as_nanos() / 100) as u64;
                data_shared.last_packet.store(qpc.max(1), Ordering::Relaxed);
                let chunk =
                    RawChunk { source: id, qpc_100ns: qpc, rate, channels, samples: data.to_vec(), silent_flag: false };
                // Never block Core Audio's real-time thread: a full queue drops the buffer.
                match tx.try_send(chunk) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        data_shared.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(TrySendError::Disconnected(_)) => data_shared.closed.store(true, Ordering::Relaxed),
                }
            },
            move |err: cpal::Error| {
                if err.kind() == cpal::ErrorKind::DeviceChanged {
                    tracing::info!("{}: audio route changed", id.as_str());
                    return;
                }
                tracing::warn!("{}: stream error: {err}", id.as_str());
                err_shared.broken.store(true, Ordering::SeqCst);
            },
            Some(Duration::from_secs(5)),
        )
        .map_err(classify)?;
    stream.play().map_err(classify)?;
    Ok(Stream {
        _stream: stream,
        rate,
        channels,
        device: device.description().ok().map(|d| d.name().to_string()),
        device_id: device.id().ok().map(|id| id.to_string()),
    })
}

/// The default device this stream follows (FR-15), if it follows one.
fn follows_default(target: &CaptureTarget) -> Option<bool> {
    match target {
        CaptureTarget::Mic { device_id: None } => Some(false),
        CaptureTarget::System => Some(true),
        _ => None,
    }
}

fn capture_thread(ctx: ThreadCtx, init: Sender<Result<(), CaptureError>>) {
    let mut shared = Arc::new(Shared::default());
    let mut stream = match open(&ctx, &shared) {
        Ok(s) => {
            let _ = init.send(Ok(()));
            s
        }
        Err(err) => {
            if matches!(err, CaptureError::MicDenied) {
                let _ = ctx.status.send(SourceStatus::MicDenied);
            }
            let _ = init.send(Err(err));
            return;
        }
    };
    let _ = ctx.status.send(SourceStatus::Started {
        source: ctx.id,
        rate: stream.rate,
        channels: stream.channels,
        device: stream.device.clone(),
    });

    let mut last_packet_seen = 0u64;
    let mut last_packet = Instant::now();
    let mut last_check = Instant::now();
    let mut change_pending: Option<Instant> = None;
    let mut known_default = follows_default(&ctx.target).and_then(devices::current_default_id);
    let mut restarted_for_silence = false;
    let mut dropped_reported = 0u64;

    'outer: while !ctx.stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
        if shared.closed.load(Ordering::Relaxed) {
            break;
        }
        let seen = shared.last_packet.load(Ordering::Relaxed);
        if seen != last_packet_seen {
            last_packet_seen = seen;
            last_packet = Instant::now();
            restarted_for_silence = false;
        }
        let mut reopen = shared.broken.swap(false, Ordering::SeqCst);

        if last_check.elapsed() >= Duration::from_secs(1) {
            last_check = Instant::now();
            let dropped = shared.dropped.load(Ordering::Relaxed);
            if dropped > dropped_reported {
                tracing::warn!("{}: {} buffers dropped (queue full)", ctx.id.as_str(), dropped - dropped_reported);
                dropped_reported = dropped;
            }
            if let Some(system) = follows_default(&ctx.target) {
                let now_default = devices::current_default_id(system);
                if now_default.is_some() && now_default != known_default {
                    known_default = now_default;
                    change_pending = Some(Instant::now());
                }
            }
            // Bluetooth headsets flap the default while switching profiles: let it settle first.
            if change_pending.is_some_and(|t| t.elapsed() >= DEFAULT_CHANGE_SETTLE) {
                change_pending = None;
                if known_default.is_some() && known_default != stream.device_id {
                    tracing::info!("{}: default device changed, reopening", ctx.id.as_str());
                    reopen = true;
                }
            }
            // A tap on the system output is legitimately quiet, but Core Audio still calls back
            // with zeros, so every source expects buffers.
            if last_packet.elapsed() >= NO_PACKETS_RESTART {
                if !restarted_for_silence {
                    tracing::warn!("{}: no buffers for 5 s, restarting once", ctx.id.as_str());
                    restarted_for_silence = true;
                    last_packet = Instant::now();
                    reopen = true;
                } else if matches!(ctx.target, CaptureTarget::Mic { .. }) {
                    let _ = ctx.status.send(SourceStatus::Failed {
                        source: ctx.id,
                        message: "マイクから音声が届いていません".into(),
                    });
                    last_packet = Instant::now();
                }
            }
        }

        if reopen {
            drop(stream);
            let mut delay = Duration::from_millis(200);
            loop {
                if ctx.stop.load(Ordering::SeqCst) {
                    break 'outer;
                }
                shared = Arc::new(Shared::default());
                match open(&ctx, &shared) {
                    Ok(s) => {
                        let _ =
                            ctx.status.send(SourceStatus::DeviceSwitched { source: ctx.id, device: s.device.clone() });
                        stream = s;
                        last_packet_seen = 0;
                        last_packet = Instant::now();
                        dropped_reported = 0;
                        break;
                    }
                    Err(err) => {
                        tracing::warn!("{}: reopen failed: {err}", ctx.id.as_str());
                        if matches!(err, CaptureError::MicDenied) {
                            let _ = ctx.status.send(SourceStatus::MicDenied);
                        }
                        std::thread::sleep(delay);
                        delay = (delay * 2).min(Duration::from_secs(2));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform;

    /// Buffers carry host times on `qpc_now_100ns`'s clock, close to now.
    #[test]
    #[ignore = "records from the real microphone"]
    fn mic_buffers_are_stamped_on_the_qpc_clock() {
        let (status_tx, _status_rx) = crossbeam_channel::unbounded();
        let (tx, rx) = crossbeam_channel::bounded(400);
        let mut source = CoreAudioSource::new(SourceId::Mic, CaptureTarget::Mic { device_id: None }, status_tx);
        source.start(tx).unwrap();
        let chunk = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let now = platform::qpc_now_100ns();
        let behind_ms = now.abs_diff(chunk.qpc_100ns) / 10_000;
        source.stop();
        assert!(behind_ms < 500, "buffer is {behind_ms} ms off");
    }
}
