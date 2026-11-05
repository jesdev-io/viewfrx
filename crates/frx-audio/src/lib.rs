//! Typed, frontend-independent transport and CPAL device adapter.
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

const RING_FRAMES: usize = 8192;
const COMMAND_CAPACITY: usize = 32;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Play { path: PathBuf },
    Seek { seconds: f64 },
    Rate { value: f64 },
    Gain { value: f32 },
    Pause,
    Resume,
    Stop,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub state: &'static str,
    pub path: Option<PathBuf>,
    pub position_seconds: f64,
    pub gain: f32,
    pub rate: f64,
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
}

struct Shared {
    generation: AtomicU64,
    rendered_generation: AtomicU64,
    position: AtomicU64,
    gain: AtomicU32,
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

#[derive(Debug, Serialize)]
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
            .name("frx-decode".into())
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
            Command::Seek { seconds }
                if !seconds.is_finite() || !(0.0..=1e9).contains(&seconds) =>
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
        rate: detail.rate,
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
        Command::Play { path } => {
            invalidate(shared);
            *renderer = None;
            shared.loading.store(true, Ordering::Release);
            shared.position.store(0f64.to_bits(), Ordering::Release);
            let rate = {
                let mut detail = shared.detail.lock().unwrap();
                detail.path = Some(path.clone());
                detail.error = None;
                detail.rate
            };
            *renderer = Some(Renderer::open(&path, sample_rate, rate)?);
            // Preserve pause across selection changes. Otherwise a queued play
            // could undo a newer, immediate pause request.
            shared.active.store(true, Ordering::Release);
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
        Command::Stop => {
            invalidate(shared);
            *renderer = None;
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

fn worker(
    receiver: mpsc::Receiver<Command>,
    mut producer: rtrb::Producer<Frame>,
    shared: Arc<Shared>,
    sample_rate: u32,
) {
    let mut renderer = None;
    while !shared.shutdown.load(Ordering::Acquire) {
        // Coalesce a consecutive burst of seeks or plays; preserve other ordering.
        let mut pending = Vec::with_capacity(COMMAND_CAPACITY);
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
        for command in pending {
            if let Err(error) = apply(command, &mut renderer, &shared, sample_rate) {
                fail(&shared, &mut renderer, error);
            }
        }
        if let Some(current) = renderer.as_mut() {
            let generation = shared.generation.load(Ordering::Acquire);
            // Limit producer work per pass so queued controls get serviced often.
            for _ in 0..producer.slots().min(256) {
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
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn fail(shared: &Shared, renderer: &mut Option<Renderer>, error: anyhow::Error) {
    invalidate(shared);
    shared.loading.store(false, Ordering::Release);
    shared.detail.lock().unwrap().error = Some(format!("{error:#}"));
    *renderer = None;
}
