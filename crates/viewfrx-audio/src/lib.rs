//! Typed, frontend-independent transport and CPAL device adapter.
pub mod config;
mod output;
pub mod render;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use render::{Renderer, validate_gain, validate_rate};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use config::*;
use viewfrx_core::{
    config::MAX_SEEK_SECONDS,
    levels::{Normalization, amplitude_dbfs},
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Play {
        path: PathBuf,
    },
    /// Atomic start with a linked-channel sample peak supplied by shared analysis.
    PlayNormalized {
        path: PathBuf,
        peak: f32,
    },
    /// Apply/clear normalization only if this path is still loaded. Stale analysis
    /// results are ignored. None restores the unnormalized source.
    Normalize {
        path: PathBuf,
        peak: Option<f32>,
    },
    Seek {
        seconds: f64,
    },
    Rate {
        value: f64,
    },
    /// Toggle the fixed highpass that removes infra-sound content. The DC
    /// blocker is always on; this is the optional additional fixed highpass.
    Highpass {
        enabled: bool,
    },
    Gain {
        value: f32,
    },
    Pause,
    Resume,
    Stop,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub state: &'static str,
    pub path: Option<PathBuf>,
    pub position_seconds: f64,
    pub gain: f32,
    pub gain_db: Option<f32>,
    pub normalization_gain: f32,
    pub normalization_enabled: bool,
    pub rms_dbfs: [Option<f32>; 2],
    pub peak_dbfs: [Option<f32>; 2],
    pub rate: f64,
    pub highpass: bool,
    pub peak: f32,
    pub underrun_callbacks: u64,
    pub clipped_samples: u64,
    pub generation: u64,
    /// Generation most recently consumed by the output callback (not DAC time).
    pub rendered_generation: u64,
    pub error: Option<String>,
}

#[derive(Default)]
struct Detail {
    path: Option<PathBuf>,
    error: Option<String>,
    rate: f64,
    highpass: bool,
}

struct Shared {
    generation: AtomicU64,
    rendered_generation: AtomicU64,
    position: AtomicU64,
    gain: AtomicU32,
    normalization_gain: AtomicU32,
    normalization_enabled: AtomicBool,
    rms: [AtomicU32; 2],
    channel_peak: [AtomicU32; 2],
    peak: AtomicU32,
    paused: AtomicBool,
    active: AtomicBool,
    loading: AtomicBool,
    done: AtomicBool,
    ended: AtomicBool,
    shutdown: AtomicBool,
    device_error: AtomicBool,
    underruns: AtomicU64,
    clipped: AtomicU64,
    detail: Mutex<Detail>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            generation: AtomicU64::new(0),
            rendered_generation: AtomicU64::new(0),
            position: AtomicU64::new(0f64.to_bits()),
            gain: AtomicU32::new(1f32.to_bits()),
            normalization_gain: AtomicU32::new(1f32.to_bits()),
            normalization_enabled: AtomicBool::new(false),
            rms: std::array::from_fn(|_| AtomicU32::new(0)),
            channel_peak: std::array::from_fn(|_| AtomicU32::new(0)),
            peak: AtomicU32::new(0),
            paused: AtomicBool::new(false),
            active: AtomicBool::new(false),
            loading: AtomicBool::new(false),
            done: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            device_error: AtomicBool::new(false),
            underruns: AtomicU64::new(0),
            clipped: AtomicU64::new(0),
            detail: Mutex::new(Detail {
                rate: 1.0,
                highpass: false,
                ..Default::default()
            }),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    samples: [f32; 2],
    position: f64,
    generation: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub index: usize,
    pub name: String,
    pub default_config: Option<String>,
}

pub fn devices() -> Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    host.output_devices()?
        .enumerate()
        .map(|(index, d)| {
            Ok(DeviceInfo {
                index,
                name: d.name()?,
                default_config: d.default_output_config().ok().map(|c| {
                    format!(
                        "{} Hz, {} channels, {:?}",
                        c.sample_rate().0,
                        c.channels(),
                        c.sample_format()
                    )
                }),
            })
        })
        .collect()
}

/// Owns the device stream and a decoder worker. Commands acknowledge queueing,
/// not completion: observe status for load/seek failure and rendered position.
pub struct Engine {
    sender: SyncSender<Command>,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    _stream: cpal::Stream,
}

impl Engine {
    pub fn open(device_index: Option<usize>) -> Result<Self> {
        let host = cpal::default_host();
        let device = if let Some(index) = device_index {
            host.output_devices()?
                .nth(index)
                .context("output device index not found; run devices")?
        } else {
            host.default_output_device()
                .context("no default output device")?
        };
        let config = device.default_output_config()?;
        let shared = Arc::new(Shared::default());
        let (producer, consumer) = rtrb::RingBuffer::new(RING_FRAMES);
        let stream = output::build_stream(&device, &config, consumer, shared.clone())?;
        stream.play()?;
        let (sender, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let worker_shared = shared.clone();
        let sample_rate = config.sample_rate().0;
        let worker = thread::Builder::new()
            .name("viewfrx-decode".into())
            .spawn(move || worker(receiver, producer, worker_shared, sample_rate))?;
        Ok(Self {
            sender,
            shared,
            worker: Some(worker),
            _stream: stream,
        })
    }

    pub fn execute(&self, command: Command) -> Result<()> {
        match command {
            Command::Gain { value } => {
                validate_gain(value)?;
                self.shared.gain.store(value.to_bits(), Ordering::Relaxed);
                return Ok(());
            }
            Command::Pause => {
                self.shared.paused.store(true, Ordering::Release);
                return Ok(());
            }
            Command::Resume => {
                self.shared.paused.store(false, Ordering::Release);
                return Ok(());
            }
            Command::Rate { value } => validate_rate(value)?,
            Command::PlayNormalized { peak, .. }
            | Command::Normalize {
                peak: Some(peak), ..
            } => {
                Normalization::from_peak(peak)?;
            }
            Command::Seek { seconds }
                if !seconds.is_finite() || !(0.0..=MAX_SEEK_SECONDS).contains(&seconds) =>
            {
                bail!("seek must be finite and between 0 and 1e9 seconds")
            }
            _ => {}
        }
        self.sender
            .try_send(command)
            .context("playback command queue full or worker unavailable")
    }

    pub fn status(&self) -> Status {
        status(&self.shared)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn status(shared: &Shared) -> Status {
    let detail = shared.detail.lock().unwrap();
    let error = if shared.device_error.load(Ordering::Acquire) {
        Some("audio device stream failed; restart session or select another device".into())
    } else {
        detail.error.clone()
    };
    let state = if error.is_some() {
        "error"
    } else if shared.loading.load(Ordering::Acquire) {
        "loading"
    } else if !shared.active.load(Ordering::Acquire) {
        "stopped"
    } else if shared.ended.load(Ordering::Acquire) {
        "ended"
    } else if shared.paused.load(Ordering::Acquire) {
        "paused"
    } else {
        "playing"
    };
    Status {
        state,
        path: detail.path.clone(),
        position_seconds: f64::from_bits(shared.position.load(Ordering::Acquire)),
        gain: f32::from_bits(shared.gain.load(Ordering::Relaxed)),
        gain_db: amplitude_dbfs(f32::from_bits(shared.gain.load(Ordering::Relaxed))),
        normalization_gain: f32::from_bits(shared.normalization_gain.load(Ordering::Relaxed)),
        normalization_enabled: shared.normalization_enabled.load(Ordering::Relaxed),
        rms_dbfs: std::array::from_fn(|i| {
            amplitude_dbfs(f32::from_bits(shared.rms[i].load(Ordering::Relaxed)))
        }),
        peak_dbfs: std::array::from_fn(|i| {
            amplitude_dbfs(f32::from_bits(
                shared.channel_peak[i].load(Ordering::Relaxed),
            ))
        }),
        rate: detail.rate,
        highpass: detail.highpass,
        peak: f32::from_bits(shared.peak.load(Ordering::Relaxed)),
        underrun_callbacks: shared.underruns.load(Ordering::Relaxed),
        clipped_samples: shared.clipped.load(Ordering::Relaxed),
        generation: shared.generation.load(Ordering::Acquire),
        rendered_generation: shared.rendered_generation.load(Ordering::Acquire),
        error,
    }
}

fn invalidate(shared: &Shared) {
    shared.active.store(false, Ordering::Release);
    shared.generation.fetch_add(1, Ordering::AcqRel);
    shared.done.store(false, Ordering::Release);
    shared.ended.store(false, Ordering::Release);
}

fn apply(
    command: Command,
    renderer: &mut Option<Renderer>,
    shared: &Shared,
    sample_rate: u32,
) -> Result<()> {
    match command {
        Command::Play { path } => load(path, None, renderer, shared, sample_rate)?,
        Command::PlayNormalized { path, peak } => {
            load(path, Some(peak), renderer, shared, sample_rate)?
        }
        Command::Normalize { path, peak } => {
            if shared.detail.lock().unwrap().path.as_ref() != Some(&path) {
                return Ok(());
            }
            set_normalization(shared, peak)?;
        }
        Command::Seek { seconds } => {
            let renderer = renderer.as_mut().context("no file loaded")?;
            invalidate(shared);
            renderer.seek(seconds)?;
            shared
                .position
                .store(renderer.position().to_bits(), Ordering::Release);
            shared.active.store(true, Ordering::Release);
        }
        Command::Rate { value } => {
            if let Some(renderer) = renderer.as_mut() {
                invalidate(shared);
                renderer.seek(f64::from_bits(shared.position.load(Ordering::Acquire)))?;
                renderer.set_rate(value)?;
                shared.active.store(true, Ordering::Release);
            }
            shared.detail.lock().unwrap().rate = value;
        }
        Command::Highpass { enabled } => {
            // Filter state is continuous, so toggling needs no seek/restart.
            if let Some(renderer) = renderer.as_mut() {
                renderer.set_highpass(enabled);
            }
            shared.detail.lock().unwrap().highpass = enabled;
        }
        Command::Stop => {
            invalidate(shared);
            *renderer = None;
            set_normalization(shared, None)?;
            shared.position.store(0f64.to_bits(), Ordering::Release);
            shared.peak.store(0, Ordering::Relaxed);
            let mut detail = shared.detail.lock().unwrap();
            detail.path = None;
            detail.error = None;
        }
        _ => unreachable!("immediate commands are handled by execute"),
    }
    shared.loading.store(false, Ordering::Release);
    shared.detail.lock().unwrap().error = None;
    Ok(())
}

fn set_normalization(shared: &Shared, peak: Option<f32>) -> Result<()> {
    let gain = peak
        .map(Normalization::from_peak)
        .transpose()?
        .map_or(1.0, |n| n.gain);
    shared
        .normalization_gain
        .store(gain.to_bits(), Ordering::Relaxed);
    shared
        .normalization_enabled
        .store(peak.is_some(), Ordering::Relaxed);
    Ok(())
}

fn load(
    path: PathBuf,
    peak: Option<f32>,
    renderer: &mut Option<Renderer>,
    shared: &Shared,
    sample_rate: u32,
) -> Result<()> {
    invalidate(shared);
    *renderer = None;
    shared.loading.store(true, Ordering::Release);
    shared.position.store(0f64.to_bits(), Ordering::Release);
    set_normalization(shared, peak)?;
    let (rate, highpass) = {
        let mut detail = shared.detail.lock().unwrap();
        detail.path = Some(path.clone());
        detail.error = None;
        (detail.rate, detail.highpass)
    };
    let mut new_renderer = Renderer::open(&path, sample_rate, rate)?;
    new_renderer.set_highpass(highpass);
    *renderer = Some(new_renderer);
    // Preserve the latest immediate pause request across a queued load.
    shared.active.store(true, Ordering::Release);
    Ok(())
}

fn worker(
    receiver: mpsc::Receiver<Command>,
    mut producer: rtrb::Producer<Frame>,
    shared: Arc<Shared>,
    sample_rate: u32,
) {
    let mut renderer = None;
    let mut pending = Vec::with_capacity(COMMAND_CAPACITY);
    while !shared.shutdown.load(Ordering::Acquire) {
        // Coalesce a consecutive burst of seeks or plays; preserve other ordering.
        for _ in 0..COMMAND_CAPACITY {
            match receiver.try_recv() {
                Ok(command) => {
                    if matches!(
                        (&command, pending.last()),
                        (Command::Seek { .. }, Some(Command::Seek { .. }))
                            | (Command::Play { .. }, Some(Command::Play { .. }))
                    ) {
                        pending.pop();
                    }
                    pending.push(command);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        for command in pending.drain(..) {
            if let Err(error) = apply(command, &mut renderer, &shared, sample_rate) {
                fail(&shared, &mut renderer, error);
            }
        }
        if let Some(current) = renderer.as_mut() {
            let generation = shared.generation.load(Ordering::Acquire);
            // Limit producer work per pass so queued controls get serviced often.
            for _ in 0..producer.slots().min(DECODE_BATCH_FRAMES) {
                match current.next_frame() {
                    Ok(Some(samples)) => {
                        let _ = producer.push(Frame {
                            samples,
                            position: current.position(),
                            generation,
                        });
                    }
                    Ok(None) => {
                        shared.done.store(true, Ordering::Release);
                        break;
                    }
                    Err(error) => {
                        fail(&shared, &mut renderer, error);
                        break;
                    }
                }
            }
        }
        if renderer.is_none() || producer.slots() == 0 || shared.done.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(WORKER_POLL_MS));
        }
    }
}

fn fail(shared: &Shared, renderer: &mut Option<Renderer>, error: anyhow::Error) {
    invalidate(shared);
    shared.loading.store(false, Ordering::Release);
    shared.detail.lock().unwrap().error = Some(format!("{error:#}"));
    *renderer = None;
}
