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

const MAX_FILES: usize = 100_000;

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
        let (tx, rx) = mpsc::sync_channel(32);
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
        let (tx, rx) = mpsc::sync_channel(32);
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
            let mut rate = 1.0;
            let mut refresh = true;
            while !stop.load(Ordering::Acquire) {
                for _ in 0..32 {
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
                ctx.request_repaint_after(Duration::from_millis(33));
                thread::sleep(Duration::from_millis(10));
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
                thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

impl LibraryService {
    pub fn start(ctx: egui::Context, catalog_path: PathBuf) -> Self {
        let job = Arc::new(Mutex::new(None::<ScanJob>));
        let jobs = job.clone();
        let generation = Arc::new(AtomicU64::new(0));
        let version = generation.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let stop = shutdown.clone();
        let (tx, rx) = mpsc::sync_channel(8);
        thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                let next = jobs.lock().unwrap().take();
                let Some(job) = next else {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                };
                let cancelled = || {
                    stop.load(Ordering::Acquire)
                        || version.load(Ordering::Acquire) != job.generation
                };
                let mut batch = Vec::with_capacity(128);
                let result = (|| -> anyhow::Result<()> {
                    let mut catalog = Catalog::load(&catalog_path)?;
                    if !job.imports.is_empty() {
                        catalog.import(&job.imports)?;
                        catalog.save(&catalog_path)?;
                    }
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
                                            "100,000-file display limit reached; narrow the imported library"
                                        );
                                    }
                                    seen.insert(path.clone());
                                    batch.push(Entry::new(path));
                                    if batch.len() == 128
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
        // Keep imports additive even when a rescan supersedes a queued scan.
        let mut job = pending.take().unwrap_or_default();
        job.imports.extend(imports);
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
}
pub struct AnalysisService {
    job: Arc<Mutex<Option<(u64, PathBuf)>>>,
    generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
    pub rx: Receiver<AnalysisEvent>,
}
impl AnalysisService {
    pub fn start(ctx: egui::Context, cache: PathBuf) -> Self {
        let job = Arc::new(Mutex::new(None::<(u64, PathBuf)>));
        let jobs = job.clone();
        let generation = Arc::new(AtomicU64::new(0));
        let version = generation.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let stop = shutdown.clone();
        let (tx, rx) = mpsc::sync_channel(4);
        thread::spawn(move || {
            let cache = WaveformCache::new(cache);
            while !stop.load(Ordering::Acquire) {
                let next = jobs.lock().unwrap().take();
                let Some((id, path)) = next else {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                };
                let cancelled =
                    || stop.load(Ordering::Acquire) || version.load(Ordering::Acquire) != id;
                let info = inspect(&path).map_err(|e| format!("{e:#}"));
                let valid = info.is_ok();
                if !deliver(&tx, AnalysisEvent::Metadata(id, info), cancelled, &ctx) || !valid {
                    continue;
                }
                let wave = cache
                    .get_cancellable(&path, 4096, cancelled)
                    .map(|c| c.waveform)
                    .map_err(|e| format!("{e:#}"));
                deliver(&tx, AnalysisEvent::Waveform(id, wave), cancelled, &ctx);
            }
        });
        Self {
            job,
            generation,
            shutdown,
            rx,
        }
    }
    pub fn request(&self, path: PathBuf) -> u64 {
        let id = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        *self.job.lock().unwrap() = Some((id, path));
        id
    }
}
impl Drop for AnalysisService {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}
