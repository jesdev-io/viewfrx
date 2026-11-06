//! Blocking adapters live here, never in egui update(). Bounded result queues and
//! generation tokens prevent old scans/analyses from replacing newer selections.
#[cfg(test)]
#[path = "services_tests.rs"]
mod tests;
use eframe::egui;
use frx_audio::{Command, DeviceInfo, Engine, Status};
use frx_core::{
    catalog::{Catalog, discover_ordered},
    decode::{AudioInfo, inspect},
    waveform::{Waveform, WaveformCache},
};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::Duration,
};

use crate::config::{controls, runtime::*};
use frx_core::config::DEFAULT_WAVEFORM_BINS;

#[derive(Clone, Default)]
pub struct AudioSnapshot {
    pub status: Option<Status>,
    pub devices: Vec<DeviceInfo>,
    pub error: Option<String>,
    pub opening: bool,
}
pub enum AudioRequest {
    Transport(Command),
    Device(Option<usize>),
    Refresh,
}
pub struct AudioService {
    tx: SyncSender<AudioRequest>,
    snapshot: Arc<Mutex<AudioSnapshot>>,
    shutdown: Arc<AtomicBool>,
}
impl AudioService {
    #[cfg(test)]
    pub fn mock() -> (Self, Receiver<AudioRequest>) {
        let (tx, rx) = mpsc::sync_channel(COMMAND_QUEUE);
        (
            Self {
                tx,
                snapshot: Arc::new(Mutex::new(AudioSnapshot::default())),
                shutdown: Arc::new(AtomicBool::new(false)),
            },
            rx,
        )
    }

    pub fn start(ctx: egui::Context, muted: bool) -> Self {
        let (tx, rx) = mpsc::sync_channel(COMMAND_QUEUE);
        let snapshot = Arc::new(Mutex::new(AudioSnapshot {
            opening: true,
            ..Default::default()
        }));
        let output = snapshot.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let stop = shutdown.clone();
        thread::spawn(move || {
            let mut engine = None;
            let mut state = AudioSnapshot::default();
            let mut desired_device = Some(None);
            let mut gain = if muted { 0.0 } else { 1.0 };
            let mut rate = controls::RATE_DEFAULT;
            let mut refresh = true;
            while !stop.load(Ordering::Acquire) {
                for _ in 0..COMMAND_QUEUE {
                    let Ok(request) = rx.try_recv() else { break };
                    match request {
                        AudioRequest::Device(index) => {
                            desired_device = Some(index);
                        }
                        AudioRequest::Refresh => refresh = true,
                        AudioRequest::Transport(command) => {
                            if let Command::Gain { value } = &command {
                                gain = *value;
                            }
                            if let Command::Rate { value } = &command {
                                rate = *value;
                            }
                            state.error = match engine.as_ref() {
                                Some(engine) => Engine::execute(engine, command)
                                    .err()
                                    .map(|e| format!("{e:#}")),
                                None => {
                                    Some("No audio output. Select a device or Retry output.".into())
                                }
                            };
                        }
                    }
                }
                if let Some(index) = desired_device.take() {
                    state.opening = true;
                    state.status = None;
                    *output.lock().unwrap() = state.clone();
                    ctx.request_repaint();
                    // Both CPAL creation and potentially slow decoder shutdown stay here.
                    engine = None;
                    match Engine::open(index) {
                        Ok(new) => {
                            let _ = new.execute(Command::Gain { value: gain });
                            let _ = new.execute(Command::Rate { value: rate });
                            engine = Some(new);
                            state.error = None;
                        }
                        Err(e) => state.error = Some(format!("Audio output: {e:#}")),
                    }
                    state.opening = false;
                }
                if refresh {
                    match frx_audio::devices() {
                        Ok(devices) => state.devices = devices,
                        Err(e) => state.error = Some(format!("Device discovery: {e:#}")),
                    }
                    refresh = false;
                }
                state.status = engine.as_ref().map(Engine::status);
                *output.lock().unwrap() = state.clone();
                ctx.request_repaint_after(Duration::from_millis(REPAINT_MS));
                thread::sleep(Duration::from_millis(POLL_MS));
            }
        });
        Self {
            tx,
            snapshot,
            shutdown,
        }
    }
    pub fn send(&self, request: AudioRequest) -> Result<(), String> {
        self.tx
            .try_send(request)
            .map_err(|_| "Audio control busy; please retry.".into())
    }
    pub fn snapshot(&self) -> Option<AudioSnapshot> {
        self.snapshot.try_lock().ok().map(|s| s.clone())
    }
}
impl Drop for AudioService {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

#[derive(Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub search: String,
}
impl Entry {
    pub fn new(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let search = path.to_string_lossy().to_lowercase();
        Self { path, name, search }
    }
}
#[derive(Default)]
struct ScanJob {
    imports: Vec<PathBuf>,
    recursive: bool,
    generation: u64,
}
pub enum ScanEvent {
    Batch(u64, Vec<Entry>),
    Done(u64, Option<String>),
}
pub struct LibraryService {
    job: Arc<Mutex<Option<ScanJob>>>,
    pub rx: Receiver<ScanEvent>,
    generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
}

fn deliver<T>(
    tx: &SyncSender<T>,
    mut event: T,
    cancelled: impl Fn() -> bool,
    ctx: &egui::Context,
) -> bool {
    loop {
        if cancelled() {
            return false;
        }
        match tx.try_send(event) {
            Ok(()) => {
                ctx.request_repaint();
                return true;
            }
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(returned)) => {
                event = returned;
                thread::sleep(Duration::from_millis(BACKPRESSURE_MS));
            }
        }
    }
}

impl LibraryService {
    pub fn start(ctx: egui::Context, _catalog_path: PathBuf) -> Self {
        let job = Arc::new(Mutex::new(None::<ScanJob>));
        let jobs = job.clone();
        let generation = Arc::new(AtomicU64::new(0));
        let version = generation.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let stop = shutdown.clone();
        let (tx, rx) = mpsc::sync_channel(SCAN_QUEUE);
        thread::spawn(move || {
            let mut roots = Vec::new();
            while !stop.load(Ordering::Acquire) {
                let next = jobs.lock().unwrap().take();
                let Some(job) = next else {
                    thread::sleep(Duration::from_millis(POLL_MS));
                    continue;
                };
                let cancelled = || {
                    stop.load(Ordering::Acquire)
                        || version.load(Ordering::Acquire) != job.generation
                };
                let mut batch = Vec::with_capacity(SCAN_BATCH);
                let result = (|| -> anyhow::Result<()> {
                    if !job.imports.is_empty() {
                        roots = job.imports.clone();
                    }
                    let mut catalog = Catalog::default();
                    catalog.import(&roots)?;
                    let mut seen = HashSet::new();
                    let mut failures = 0;
                    for root in &catalog.roots {
                        let discovery = discover_ordered(root, job.recursive, true, |entry| {
                            if cancelled() {
                                anyhow::bail!("scan cancelled");
                            }
                            match entry {
                                Ok(path) if !seen.contains(&path) => {
                                    if seen.len() >= MAX_FILES {
                                        anyhow::bail!(
                                            "{MAX_FILES}-file display limit reached; narrow the imported library"
                                        );
                                    }
                                    seen.insert(path.clone());
                                    batch.push(Entry::new(path));
                                    if batch.len() == SCAN_BATCH
                                        && !deliver(
                                            &tx,
                                            ScanEvent::Batch(
                                                job.generation,
                                                std::mem::take(&mut batch),
                                            ),
                                            cancelled,
                                            &ctx,
                                        )
                                    {
                                        anyhow::bail!("scan cancelled");
                                    }
                                }
                                Err(_) => failures += 1,
                                _ => {}
                            }
                            Ok(())
                        });
                        if let Err(e) = discovery {
                            if cancelled() || seen.len() >= MAX_FILES {
                                return Err(e);
                            }
                            failures += 1;
                        }
                    }
                    if failures > 0 {
                        anyhow::bail!(
                            "{failures} paths could not be read; check disconnected drives or permissions"
                        );
                    }
                    Ok(())
                })();
                if !batch.is_empty() {
                    deliver(
                        &tx,
                        ScanEvent::Batch(job.generation, batch),
                        cancelled,
                        &ctx,
                    );
                }
                deliver(
                    &tx,
                    ScanEvent::Done(job.generation, result.err().map(|e| format!("{e:#}"))),
                    cancelled,
                    &ctx,
                );
            }
        });
        Self {
            job,
            rx,
            generation,
            shutdown,
        }
    }
    pub fn request(&self, imports: Vec<PathBuf>, recursive: bool) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let mut pending = self.job.lock().unwrap();
        // A folder replaces the session; refresh retains the current roots.
        let mut job = pending.take().unwrap_or_default();
        if !imports.is_empty() {
            job.imports = imports;
        }
        job.recursive = recursive;
        job.generation = generation;
        *pending = Some(job);
        generation
    }
}
impl Drop for LibraryService {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

pub enum AnalysisEvent {
    Metadata(u64, Result<AudioInfo, String>),
    Waveform(u64, Result<Waveform, String>),
    #[cfg(feature = "spectrogram")]
    Spectrogram(
        u64,
        Result<Option<crate::spectral::RenderedSpectrum>, String>,
    ),
}
#[derive(Default)]
pub struct AnalysisOptions {
    #[cfg(feature = "spectrogram")]
    pub spectral: Option<crate::spectral::RenderSettings>,
}
struct AnalysisJob {
    id: u64,
    path: PathBuf,
    #[cfg(feature = "spectrogram")]
    options: AnalysisOptions,
}
pub struct AnalysisService {
    job: Arc<Mutex<Option<AnalysisJob>>>,
    generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
    pub rx: Receiver<AnalysisEvent>,
}
impl AnalysisService {
    pub fn start(ctx: egui::Context, cache: PathBuf) -> Self {
        let job = Arc::new(Mutex::new(None::<AnalysisJob>));
        let jobs = job.clone();
        let generation = Arc::new(AtomicU64::new(0));
        let version = generation.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let stop = shutdown.clone();
        let (tx, rx) = mpsc::sync_channel(ANALYSIS_QUEUE);
        thread::spawn(move || {
            #[cfg(feature = "spectrogram")]
            let spectral_cache =
                frx_core::spectrogram::SpectrogramCache::new(cache.join("spectrograms"));
            let cache = WaveformCache::new(cache);
            while !stop.load(Ordering::Acquire) {
                let next = jobs.lock().unwrap().take();
                let Some(job) = next else {
                    thread::sleep(Duration::from_millis(POLL_MS));
                    continue;
                };
                let id = job.id;
                let path = job.path;
                let cancelled =
                    || stop.load(Ordering::Acquire) || version.load(Ordering::Acquire) != id;
                let info = inspect(&path).map_err(|e| format!("{e:#}"));
                #[cfg(feature = "spectrogram")]
                let stereo = info.as_ref().is_ok_and(|i| i.channels == 2);
                let valid = info.is_ok();
                if !deliver(&tx, AnalysisEvent::Metadata(id, info), cancelled, &ctx) || !valid {
                    continue;
                }
                let wave = cache
                    .get_cancellable(&path, DEFAULT_WAVEFORM_BINS, cancelled)
                    .map(|c| c.waveform)
                    .map_err(|e| format!("{e:#}"));
                if !deliver(&tx, AnalysisEvent::Waveform(id, wave), cancelled, &ctx) {
                    continue;
                }
                #[cfg(feature = "spectrogram")]
                if let Some(settings) = job.options.spectral {
                    let spectrum = (|| -> anyhow::Result<_> {
                        let cached = spectral_cache.get_channel(
                            &path,
                            settings.analysis,
                            Some(0),
                            cancelled,
                        )?;
                        let mut rendered = crate::spectral::render(cached.spectrogram, settings);
                        if stereo && let Some(left) = &mut rendered {
                            let cached = spectral_cache.get_channel(
                                &path,
                                settings.analysis,
                                Some(1),
                                cancelled,
                            )?;
                            left.right = crate::spectral::render(cached.spectrogram, settings)
                                .map(|r| r.image);
                        }
                        Ok(rendered)
                    })()
                    .map_err(|e| format!("{e:#}"));
                    deliver(
                        &tx,
                        AnalysisEvent::Spectrogram(id, spectrum),
                        cancelled,
                        &ctx,
                    );
                }
            }
        });
        Self {
            job,
            generation,
            shutdown,
            rx,
        }
    }
    #[cfg(test)]
    pub fn request(&self, path: PathBuf) -> u64 {
        self.request_with_options(path, AnalysisOptions::default())
    }

    pub fn request_with_options(&self, path: PathBuf, _options: AnalysisOptions) -> u64 {
        let id = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        *self.job.lock().unwrap() = Some(AnalysisJob {
            id,
            path,
            #[cfg(feature = "spectrogram")]
            options: _options,
        });
        id
    }
}
impl Drop for AnalysisService {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}
