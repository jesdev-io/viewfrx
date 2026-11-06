//! UI state and routing only. Paint/layout lives in panels/widgets; audio and
//! filesystem work is delegated to services backed by the headless crates.
#[path = "panels.rs"]
mod panels;
#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;

#[cfg(test)]
use crate::widgets::waveform_view;
use crate::{
    Args,
    config::{controls as c, layout as l, runtime as r},
    model::{Browser, seek_at},
    services::{
        AnalysisEvent, AnalysisOptions, AnalysisService, AudioRequest, AudioService, AudioSnapshot,
        FileEvent, FileService, LibraryService, ScanEvent,
    },
    theme,
};
use eframe::egui::{self, RichText};
#[cfg(test)]
use eframe::egui::{pos2, vec2};
use frx_audio::Command;
use frx_core::{
    annotations::{self, Annotation},
    decode::AudioInfo,
    levels::db_to_gain,
    waveform::Waveform,
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

pub struct App {
    audio: AudioService,
    snapshot: AudioSnapshot,
    library: LibraryService,
    analysis: AnalysisService,
    files: FileService,
    file_id: u64,
    annotations: Vec<Annotation>,
    annotation_text: String,
    highpass: bool,
    debug_console: bool,
    browser: Browser,
    scan_id: u64,
    analysis_id: u64,
    scanning: bool,
    scroll_to_selection: bool,
    analyzing: bool,
    recursive: bool,
    autoplay: bool,
    info: Option<AudioInfo>,
    waveform: Option<Waveform>,
    wave_channels: crate::widgets::Channels,
    error: Option<String>,
    gain_db: f64,
    muted: bool,
    normalize: bool,
    pending_play: bool,
    pending_seek: Option<f64>,
    normalization_applied: Option<(PathBuf, u64, bool)>,
    rate: f64,
    device: Option<usize>,
    picker: Option<Receiver<Vec<PathBuf>>>,
    logo: egui::TextureHandle,
    seek_sent: Instant,
    #[cfg(feature = "spectrogram")]
    spectral: crate::spectral::View,
    started: Instant,
    smoke_seconds: Option<f64>,
    smoke_reported: bool,
    frame_count: u64,
    max_update_ms: f64,
    screenshot: Option<PathBuf>,
    capture_requested: bool,
    capture_done: Option<Receiver<Result<(), String>>>,
}

impl App {
    pub fn new(ctx: &egui::Context, args: Args) -> Self {
        let audio = AudioService::start(ctx.clone(), args.muted);
        Self::with_audio(ctx, args, audio)
    }
    fn with_audio(ctx: &egui::Context, args: Args, audio: AudioService) -> Self {
        theme::apply(ctx);
        let image = theme::logo();
        let logo = ctx.load_texture(
            "jesdev",
            egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            ),
            Default::default(),
        );
        let library = LibraryService::start(ctx.clone(), args.library);
        let scan_id = library.request(args.paths, c::RECURSIVE_DEFAULT);
        Self {
            audio,
            snapshot: AudioSnapshot::default(),
            analysis: AnalysisService::start(ctx.clone(), args.cache),
            files: FileService::start(ctx.clone()),
            file_id: 0,
            annotations: Vec::new(),
            annotation_text: String::new(),
            highpass: c::HIGHPASS_DEFAULT,
            debug_console: args.debug_console,
            library,
            browser: Browser::default(),
            scan_id,
            analysis_id: 0,
            scanning: true,
            scroll_to_selection: false,
            analyzing: false,
            recursive: c::RECURSIVE_DEFAULT,
            autoplay: c::AUTOPLAY_DEFAULT,
            info: None,
            waveform: None,
            wave_channels: Default::default(),
            error: None,
            gain_db: c::GAIN_DEFAULT_DB,
            muted: args.muted,
            normalize: c::NORMALIZE_DEFAULT,
            pending_play: false,
            pending_seek: None,
            normalization_applied: None,
            rate: c::RATE_DEFAULT,
            device: None,
            picker: None,
            logo,
            seek_sent: Instant::now(),
            #[cfg(feature = "spectrogram")]
            spectral: Default::default(),
            started: Instant::now(),
            smoke_seconds: args.smoke_seconds,
            smoke_reported: false,
            frame_count: 0,
            max_update_ms: 0.0,
            screenshot: args.screenshot,
            capture_requested: false,
            capture_done: None,
        }
    }
    fn analysis_options(&self) -> AnalysisOptions {
        AnalysisOptions {
            #[cfg(feature = "spectrogram")]
            spectral: self.spectral.settings(),
        }
    }
    fn request_analysis(&mut self, path: PathBuf) {
        self.analysis_id = self
            .analysis
            .request_with_options(path, self.analysis_options());
        self.analyzing = true;
        #[cfg(feature = "spectrogram")]
        {
            self.spectral.pending = self.spectral.enabled;
        }
    }
    fn send(&mut self, command: Command) {
        crate::logln!("transport: {command:?}");
        if let Err(error) = self.audio.send(AudioRequest::Transport(command)) {
            crate::logln!("transport rejected: {error}");
            self.error = Some(error);
        }
    }
    fn send_gain(&mut self) {
        self.send(Command::Gain {
            value: if self.muted {
                0.0
            } else {
                db_to_gain(self.gain_db as f32)
            },
        });
    }
    fn start_selected(&mut self) -> bool {
        let Some(path) = self.browser.selected.clone() else {
            return false;
        };
        let command = if self.normalize {
            match self.waveform.as_ref().map(Waveform::normalization) {
                None => {
                    if !self.analyzing {
                        self.error = None;
                        self.request_analysis(path.clone());
                    }
                    self.pending_play = true;
                    self.send(Command::Stop);
                    return false;
                }
                Some(Err(error)) => {
                    self.error = Some(error.to_string());
                    self.stop();
                    return false;
                }
                Some(Ok(n)) => Command::PlayNormalized {
                    path,
                    peak: n.source_peak,
                },
            }
        } else {
            Command::Play { path }
        };
        self.pending_play = false;
        self.send(command);
        true
    }
    fn stop(&mut self) {
        self.pending_play = false;
        self.pending_seek = None;
        self.send(Command::Stop);
    }
    fn finish_pending_play(&mut self) {
        if !self.pending_play || (self.normalize && self.waveform.is_none()) {
            return;
        }
        if self.start_selected() {
            if let Some(seconds) = self.pending_seek.take() {
                self.send(Command::Seek { seconds });
            }
            self.send(Command::Resume);
        }
    }
    fn normalization_changed(&mut self) {
        self.normalization_applied = None;
        if self.normalize
            && let Some(Err(error)) = self.waveform.as_ref().map(Waveform::normalization)
        {
            self.error = Some(error.to_string());
            self.stop();
            return;
        }
        if self.normalize
            && self.waveform.is_none()
            && self
                .snapshot
                .status
                .as_ref()
                .is_some_and(|s| s.path == self.browser.selected && s.state == "playing")
        {
            self.pending_seek = Some(self.position());
            self.start_selected();
        }
        self.finish_pending_play();
        self.sync_normalization();
    }
    fn sync_normalization(&mut self) {
        let Some(status) = &self.snapshot.status else {
            return;
        };
        let Some(path) = self
            .browser
            .selected
            .clone()
            .filter(|p| status.path.as_ref() == Some(p))
        else {
            return;
        };
        let key = (path.clone(), status.generation, self.normalize);
        if self.normalization_applied.as_ref() == Some(&key) {
            return;
        }
        let peak = if self.normalize {
            let Some(normalization) = self.waveform.as_ref().and_then(|w| w.normalization().ok())
            else {
                return;
            };
            Some(normalization.source_peak)
        } else {
            None
        };
        match self
            .audio
            .send(AudioRequest::Transport(Command::Normalize { path, peak }))
        {
            Ok(()) => self.normalization_applied = Some(key),
            Err(error) => self.error = Some(error),
        }
    }
    fn select(&mut self, path: PathBuf, audition: bool) {
        crate::logln!("select {} (audition={audition})", path.display());
        self.request_analysis(path.clone());
        self.browser.selected = Some(path);
        self.info = None;
        self.waveform = None;
        self.annotations.clear();
        self.annotation_text.clear();
        self.error = None;
        self.normalization_applied = None;
        self.pending_play = false;
        self.pending_seek = None;
        #[cfg(feature = "spectrogram")]
        self.spectral.clear();
        if audition && self.start_selected() {
            self.send(Command::Resume);
        }
    }
    fn navigate(&mut self, direction: i32) {
        if let Some(path) = self.browser.adjacent(direction) {
            self.select(path, self.autoplay);
            self.scroll_to_selection = true;
        }
    }
    fn rescan(&mut self, paths: Vec<PathBuf>) {
        crate::logln!("rescan with {} new path(s)", paths.len());
        self.pending_play = false;
        self.pending_seek = None;
        self.scan_id = self.library.request(paths, self.recursive);
        self.browser.clear();
        self.scanning = true;
        self.error = None;
    }
    fn poll(&mut self, ctx: &egui::Context) {
        #[cfg(not(feature = "spectrogram"))]
        let _ = ctx;
        if let Some(snapshot) = self.audio.snapshot() {
            self.snapshot = snapshot;
        }
        for _ in 0..r::SCAN_QUEUE {
            let Ok(event) = self.library.rx.try_recv() else {
                break;
            };
            match event {
                ScanEvent::Batch(id, entries) if id == self.scan_id => {
                    self.browser.append(entries);
                    if self.browser.selected.is_none()
                        && let Some(entry) = self.browser.entries.first()
                    {
                        self.select(entry.path.clone(), false);
                    }
                }
                ScanEvent::Done(id, error) if id == self.scan_id => {
                    self.scanning = false;
                    if let Some(error) = &error {
                        crate::logln!("scan finished with error: {error}");
                    } else {
                        crate::logln!("scan finished: {} candidates", self.browser.entries.len());
                    }
                    if error.is_some() {
                        self.error = error;
                    }
                }
                _ => {}
            }
        }
        for _ in 0..r::ANALYSIS_QUEUE {
            let Ok(event) = self.analysis.rx.try_recv() else {
                break;
            };
            match event {
                AnalysisEvent::Metadata(id, result) if id == self.analysis_id => match result {
                    Ok(info) => {
                        // Annotations live in the file's own metadata tags.
                        self.annotations = annotations::from_tags(&info.tags);
                        self.annotation_text = annotations::format_annotations(&self.annotations);
                        self.info = Some(info);
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.pending_play = false;
                        self.pending_seek = None;
                        self.analyzing = false;
                        #[cfg(feature = "spectrogram")]
                        {
                            self.spectral.pending = false;
                        }
                    }
                },
                AnalysisEvent::Waveform(id, result) if id == self.analysis_id => {
                    self.analyzing = false;
                    self.normalization_applied = None;
                    match result {
                        Ok(waveform) => self.waveform = Some(waveform),
                        Err(error) => {
                            self.error = Some(error);
                            self.pending_play = false;
                            self.pending_seek = None;
                        }
                    }
                }
                #[cfg(feature = "spectrogram")]
                AnalysisEvent::Spectrogram(id, result) if id == self.analysis_id => {
                    if let Err(error) = self.spectral.accept(ctx, result) {
                        self.error = Some(error);
                    }
                }
                _ => {}
            }
        }
        if let Some(rx) = &self.picker {
            match rx.try_recv() {
                Ok(paths) => {
                    self.picker = None;
                    if !paths.is_empty() {
                        self.rescan(paths);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.picker = None;
                    self.error = Some("File chooser closed unexpectedly. You can also drag files into the window.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        for _ in 0..r::ANALYSIS_QUEUE {
            let Ok(FileEvent::AnnotationsWritten(id, result)) = self.files.rx.try_recv() else {
                break;
            };
            if id != self.file_id {
                continue;
            }
            match result {
                Ok(list) => {
                    crate::logln!("annotations written: {} marker(s)", list.len());
                    self.annotations = list;
                    self.annotation_text = annotations::format_annotations(&self.annotations);
                }
                Err(error) => {
                    crate::logln!("annotation write failed: {error}");
                    self.error = Some(error);
                }
            }
        }
        self.finish_pending_play();
        self.sync_normalization();
    }
    /// Merge annotations (from the text field or a right-click) into the
    /// selected file's metadata on the file worker; idempotent.
    fn add_annotations(&mut self, new: Vec<Annotation>) {
        let Some(path) = self.browser.selected.clone() else {
            return;
        };
        if new.is_empty() {
            return;
        }
        self.file_id = self.files.write_annotations(path, new);
    }
    /// Bottom text field: "[dd:hh:mm:ss] note" entries become annotations.
    fn submit_annotation_text(&mut self) {
        match annotations::parse_text(&self.annotation_text) {
            Ok(new) => self.add_annotations(new),
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }
    fn choose(&mut self, folder: bool, ctx: &egui::Context) {
        let (tx, rx) = mpsc::sync_channel(1);
        self.picker = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let paths = if folder {
                rfd::FileDialog::new()
                    .set_title("Import audio folder")
                    .pick_folder()
                    .into_iter()
                    .collect()
            } else {
                rfd::FileDialog::new()
                    .set_title("Import audio files")
                    .add_filter("Audio", frx_core::catalog::EXTENSIONS)
                    .pick_files()
                    .unwrap_or_default()
            };
            let _ = tx.send(paths);
            ctx.request_repaint();
        });
    }
    fn duration(&self) -> f64 {
        self.info
            .as_ref()
            .and_then(|i| i.duration_seconds)
            .or_else(|| {
                self.waveform
                    .as_ref()
                    .map(|w| w.frames as f64 / w.sample_rate as f64)
            })
            .unwrap_or(0.0)
    }
    fn position(&self) -> f64 {
        self.snapshot
            .status
            .as_ref()
            .filter(|s| s.path == self.browser.selected)
            .map_or(0.0, |s| s.position_seconds)
    }
    fn toggle_play(&mut self) {
        if self.pending_play {
            self.stop();
            return;
        }
        if self.normalize && self.waveform.is_none() {
            self.pending_seek = Some(self.position());
            self.start_selected();
            return;
        }
        let Some(path) = self.browser.selected.clone() else {
            return;
        };
        match self
            .snapshot
            .status
            .as_ref()
            .filter(|s| s.path.as_ref() == Some(&path))
            .map(|s| s.state)
        {
            Some("playing") => self.send(Command::Pause),
            Some("paused") => self.send(Command::Resume),
            _ => {
                if self.start_selected() {
                    self.send(Command::Resume);
                }
            }
        }
    }
    fn seek(&mut self, seconds: f64) {
        if self.pending_play {
            self.pending_seek = Some(seconds);
            return;
        }
        let Some(path) = self.browser.selected.clone() else {
            return;
        };
        if !self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.path.as_ref() == Some(&path) && s.state != "error")
            && !self.start_selected()
        {
            if self.pending_play {
                self.pending_seek = Some(seconds);
            }
            return;
        }
        self.send(Command::Seek { seconds });
        self.seek_sent = Instant::now();
    }
    fn handle_seek(&mut self, response: egui::Response) {
        if response.double_clicked() {
            self.seek(0.0);
            return;
        }
        // Right-click at a time position creates a timestamp annotation there.
        if response.secondary_clicked()
            && let Some(pointer) = response.interact_pointer_pos()
            && let Some(seconds) = seek_at(
                (pointer.x - response.rect.left()) / response.rect.width(),
                self.duration(),
            )
        {
            self.add_annotations(vec![Annotation {
                seconds,
                text: String::new(),
            }]);
            return;
        }
        if (response.clicked()
            || response.drag_stopped()
            || (response.dragged()
                && self.seek_sent.elapsed() > Duration::from_millis(r::SEEK_THROTTLE_MS)))
            && let Some(pointer) = response.interact_pointer_pos()
            && let Some(seconds) = seek_at(
                (pointer.x - response.rect.left()) / response.rect.width(),
                self.duration(),
            )
        {
            self.seek(seconds);
        }
    }
    fn keyboard(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_play();
        }
        let direction = ctx.input(|i| {
            if i.key_pressed(egui::Key::ArrowDown) {
                1
            } else if i.key_pressed(egui::Key::ArrowUp) {
                -1
            } else {
                0
            }
        });
        if direction != 0 {
            self.navigate(direction);
        }
        let delta = ctx.input(|i| {
            if i.key_pressed(egui::Key::ArrowRight) {
                r::KEY_SEEK_SECONDS
            } else if i.key_pressed(egui::Key::ArrowLeft) {
                -r::KEY_SEEK_SECONDS
            } else {
                0.0
            }
        });
        if delta != 0.0 && self.duration() > 0.0 {
            self.seek((self.position() + delta).clamp(0.0, self.duration()));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.stop();
        }
    }
    fn diagnostic(&mut self, ctx: &egui::Context) {
        let Some(seconds) = self.smoke_seconds else {
            return;
        };
        let elapsed = self.started.elapsed().as_secs_f64();
        if elapsed >= seconds && !self.smoke_reported {
            self.smoke_reported = true;
            #[cfg(feature = "spectrogram")]
            let spectral_ready = self.spectral.has_image();
            #[cfg(not(feature = "spectrogram"))]
            let spectral_ready = false;
            println!(
                "{}",
                serde_json::json!({ "library_candidates": self.browser.entries.len(), "visible_count": self.browser.visible.len(),
                "scanning": self.scanning, "analyzing": self.analyzing, "waveform_bins": self.waveform.as_ref().map_or(0, |w| w.peaks.len()),
                "spectrogram_feature": cfg!(feature = "spectrogram"), "spectrogram_ready": spectral_ready,
                "frames": self.frame_count, "max_update_cpu_ms": self.max_update_ms })
            );
        }
        if self.screenshot.is_some() && !self.capture_requested && elapsed >= seconds {
            self.capture_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let images = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        if self.capture_done.is_none()
            && let Some(image) = images.first()
            && let Some(path) = self.screenshot.take()
        {
            let (tx, rx) = mpsc::sync_channel(1);
            self.capture_done = Some(rx);
            let image = image.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                let result = image::save_buffer(
                    path,
                    &bytes,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                )
                .map_err(|e| e.to_string());
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
        if let Some(rx) = &self.capture_done
            && let Ok(result) = rx.try_recv()
        {
            if let Err(error) = result {
                crate::SMOKE_FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
                eprintln!("Screenshot failed: {error}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if elapsed >= seconds && !self.capture_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if elapsed > seconds + r::CAPTURE_TIMEOUT_SECONDS {
            crate::SMOKE_FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
            eprintln!("Screenshot timeout");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(r::REPAINT_MS));
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let started = Instant::now();
        self.poll(ctx);
        self.keyboard(ctx);
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect::<Vec<_>>()
        });
        if !dropped.is_empty() {
            self.rescan(dropped);
        }
        self.header(ctx);
        self.footer(ctx);
        self.browser_panel(ctx);
        self.settings(ctx);
        self.center(ctx);
        if self.debug_console {
            egui::Window::new("Debug console")
                .default_size(l::DEBUG_WINDOW_SIZE)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in crate::logbuf::lines() {
                                ui.monospace(RichText::new(line).size(l::MICRO_FONT));
                            }
                        });
                });
        }
        self.diagnostic(ctx);
        self.frame_count += 1;
        self.max_update_ms = self
            .max_update_ms
            .max(started.elapsed().as_secs_f64() * 1000.0);
    }
}
