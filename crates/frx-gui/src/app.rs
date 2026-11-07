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
    model::{Browser, TimeWindow},
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
    /// What is actually stored in the file's metadata right now.
    saved_annotations: Vec<Annotation>,
    annotations_dirty: bool,
    annotation_text: String,
    /// Selection waiting on the unsaved-annotations warning.
    pending_selection: Option<(PathBuf, bool)>,
    save_picker: Option<Receiver<Option<PathBuf>>>,
    /// Proceed to pending_selection after the save dialog completes.
    resume_selection: bool,
    highpass: bool,
    debug_console: bool,
    /// Shared time-zoom window (None = full recording) for all plots.
    zoom: Option<TimeWindow>,
    /// Where playback was last started (grey marker line in the plots).
    last_play_start: Option<f64>,
    /// Focus the annotation text field next frame (after a waveform click).
    focus_annotation_field: bool,
    /// User-draggable plot heights (session-local, never persisted).
    wave_height: f32,
    spec_height: f32,
    #[cfg(feature = "spectrogram")]
    spectrum_on: bool,
    #[cfg(feature = "spectrogram")]
    spectrum_height: f32,
    /// Annotation column width as a fraction of the bottom area; the vertical
    /// split bar between the annotation and spectrum columns redistributes it.
    #[cfg(feature = "spectrogram")]
    annotation_fraction: f32,
    #[cfg(feature = "spectrogram")]
    spectrum: Option<(f64, frx_core::spectrum::Spectrum)>,
    #[cfg(feature = "spectrogram")]
    spectrum_id: u64,
    #[cfg(feature = "spectrogram")]
    last_spectrum_pos: f64,
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
            saved_annotations: Vec::new(),
            annotations_dirty: false,
            annotation_text: String::new(),
            pending_selection: None,
            save_picker: None,
            resume_selection: false,
            highpass: c::HIGHPASS_DEFAULT,
            debug_console: args.debug_console,
            zoom: None,
            last_play_start: None,
            focus_annotation_field: false,
            wave_height: 0.0,
            spec_height: 0.0,
            #[cfg(feature = "spectrogram")]
            spectrum_on: c::SPECTRUM_DEFAULT,
            #[cfg(feature = "spectrogram")]
            spectrum_height: 0.0,
            #[cfg(feature = "spectrogram")]
            annotation_fraction: 0.0,
            #[cfg(feature = "spectrogram")]
            spectrum: None,
            #[cfg(feature = "spectrogram")]
            spectrum_id: 0,
            #[cfg(feature = "spectrogram")]
            last_spectrum_pos: -1.0,
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
        self.last_play_start = Some(self.pending_seek.unwrap_or(0.0));
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
        // Unsaved floating annotations: warn before switching files.
        if self.annotations_dirty {
            crate::logln!(
                "unsaved annotations; asking before switching to {}",
                path.display()
            );
            self.pending_selection = Some((path, audition));
            return;
        }
        self.select_now(path, audition);
    }
    fn select_now(&mut self, path: PathBuf, audition: bool) {
        crate::logln!("select {} (audition={audition})", path.display());
        self.request_analysis(path.clone());
        self.browser.selected = Some(path);
        self.info = None;
        self.waveform = None;
        self.annotations.clear();
        self.annotation_text.clear();
        self.zoom = None;
        self.last_play_start = None;
        #[cfg(feature = "spectrogram")]
        {
            self.spectrum = None;
            self.last_spectrum_pos = -1.0;
        }
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
                        // Annotations found on the file are rendered; new ones
                        // stay floating (unsaved) until Ctrl+S / Save. Never
                        // clobber floating edits when metadata arrives late.
                        let stored = annotations::from_tags(&info.tags);
                        self.saved_annotations = stored.clone();
                        if !self.annotations_dirty {
                            self.annotations = stored;
                            self.annotation_text =
                                annotations::format_annotations(&self.annotations);
                        }
                        self.annotations_dirty = self.annotations != self.saved_annotations;
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
                #[cfg(feature = "spectrogram")]
                AnalysisEvent::Spectrum(id, position, result) if id == self.spectrum_id => {
                    match result {
                        Ok(spectrum) => self.spectrum = Some((position, spectrum)),
                        Err(error) => self.error = Some(error),
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
            let event = self.files.rx.try_recv();
            let Ok(event) = event else { break };
            match event {
                FileEvent::AnnotationsSaved(id, result) if id == self.file_id => match result {
                    Ok((dst, list)) => {
                        crate::logln!(
                            "annotations saved to {} ({} marker(s))",
                            dst.display(),
                            list.len()
                        );
                        let same =
                            self.browser.selected.as_ref().is_some_and(|src| {
                                src.canonicalize().ok() == dst.canonicalize().ok()
                            });
                        self.annotations = list.clone();
                        self.saved_annotations = list;
                        self.annotations_dirty = false;
                        self.annotation_text = annotations::format_annotations(&self.annotations);
                        if same && let Some(path) = self.browser.selected.clone() {
                            self.request_analysis(path); // refresh tags from disk
                        }
                        if self.resume_selection {
                            self.resume_selection = false;
                            self.proceed_pending_selection();
                        }
                    }
                    Err(error) => {
                        crate::logln!("annotation save failed: {error}");
                        self.error = Some(error);
                        self.resume_selection = false;
                    }
                },
                _ => {}
            }
        }
        if let Some(rx) = &self.save_picker {
            match rx.try_recv() {
                Ok(Some(dst)) => {
                    self.save_picker = None;
                    self.save_annotations_to(dst);
                }
                Ok(None) => {
                    self.save_picker = None; // dialog cancelled: nothing saved
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.save_picker = None;
                    self.error = Some("Save dialog closed unexpectedly.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        #[cfg(feature = "spectrogram")]
        if self.spectrum_on && !self.analyzing {
            // Never let live spectrum requests delay the initial
            // metadata/waveform preview of a fresh selection.
            let playing = self
                .snapshot
                .status
                .as_ref()
                .is_some_and(|s| s.state == "playing" && s.path == self.browser.selected);
            if playing && let Some(path) = self.browser.selected.clone() {
                let pos = self.position();
                if (pos - self.last_spectrum_pos).abs() >= c::SPECTRUM_UPDATE_SECONDS {
                    self.last_spectrum_pos = pos;
                    self.spectrum_id =
                        self.analysis
                            .request_spectrum(path, self.spectral.fft_size(), pos);
                }
            }
        }
        self.finish_pending_play();
        self.sync_normalization();
    }
    /// Merge annotations (from the text field or a right-click) into the
    /// current floating list; idempotent. Nothing is written to disk until
    /// the user saves (Ctrl+S / File menu) — writes go through save dialog.
    fn add_annotations(&mut self, new: Vec<Annotation>, sync_field: bool) {
        if new.is_empty() || self.browser.selected.is_none() {
            return;
        }
        let before = self.annotations.clone();
        annotations::merge(&mut self.annotations, &new);
        if self.annotations != before {
            self.annotations_dirty = self.annotations != self.saved_annotations;
            if sync_field {
                // Right-click markers render into the text field too (idempotent).
                let mut text = self.annotation_text.clone();
                for line in annotations::format_annotations(&new).lines() {
                    if !text.lines().any(|l| l == line) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(line);
                    }
                }
                self.annotation_text = text;
            }
            crate::logln!(
                "annotation added (floating, {} unsaved change(s))",
                self.annotations
                    .len()
                    .saturating_sub(self.saved_annotations.len())
            );
        }
    }
    /// Automatic parsing: every complete [dd:hh:mm:ss] marker in the text
    /// field immediately becomes a floating annotation (idempotent). The
    /// field itself is never rewritten while typing.
    fn sync_annotations_from_text(&mut self) {
        if let Ok(parsed) = annotations::parse_text(&self.annotation_text) {
            self.add_annotations(parsed, false);
        }
    }
    /// Open the standard save dialog (file naming + native overwrite warning)
    /// to intentionally persist the floating annotations.
    fn request_save_dialog(&mut self, ctx: &egui::Context) {
        let Some(src) = self.browser.selected.clone() else {
            return;
        };
        if self.save_picker.is_some() {
            return;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        self.save_picker = Some(rx);
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title("Save annotations into audio file")
                .set_file_name(name)
                .add_filter("Audio", frx_core::catalog::EXTENSIONS)
                .save_file();
            let _ = tx.send(picked);
            ctx.request_repaint();
        });
    }
    /// Persist the floating annotations to dst (in place when dst == source).
    fn save_annotations_to(&mut self, dst: PathBuf) {
        let Some(src) = self.browser.selected.clone() else {
            return;
        };
        self.file_id = self
            .files
            .save_annotations_as(src, dst, self.annotations.clone());
    }
    fn proceed_pending_selection(&mut self) {
        if let Some((path, audition)) = self.pending_selection.take() {
            self.select_now(path, audition);
        }
    }
    /// Bottom text field: "[dd:hh:mm:ss] note" entries become annotations.
    #[cfg(test)]
    fn submit_annotation_text(&mut self) {
        match annotations::parse_text(&self.annotation_text) {
            Ok(new) => self.add_annotations(new, false),
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
    /// Shared zoom window across all waveforms and spectrograms.
    fn window(&self) -> TimeWindow {
        self.zoom
            .unwrap_or_else(|| TimeWindow::full(self.duration()))
            .clamped(self.duration())
    }
    fn zoom_at(&mut self, center_fraction: f32, factor: f64) {
        let duration = self.duration();
        let window = self.window();
        self.zoom = if factor < 1.0 {
            Some(window.zoom_in(
                center_fraction,
                1.0 / factor.max(1e-9),
                c::ZOOM_MIN_SECONDS,
                duration,
            ))
        } else {
            window.zoom_out(center_fraction, factor, duration)
        };
        crate::logln!(
            "zoom {:?} (window {:.3}–{:.3} s of {:.3})",
            self.zoom.is_some(),
            self.window().start,
            self.window().end,
            duration
        );
    }
    fn handle_seek(&mut self, response: egui::Response, ui: &egui::Ui) {
        if response.double_clicked() {
            self.seek(0.0);
            self.zoom = None; // double-click also resets the zoom to the full view
            return;
        }
        // latest_pos works for hover (scroll zoom) and clicks alike;
        // interact_pointer_pos is None while no button is held.
        let fraction = |ui: &egui::Ui| {
            ui.input(|i| i.pointer.latest_pos())
                .map(|p| (p.x - response.rect.left()) / response.rect.width())
                .filter(|f| f.is_finite())
        };
        // Scroll wheel over a plot zooms in/out around the pointer (tandem).
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.raw_scroll_delta.y);
            if scroll > 0.0 {
                if let Some(f) = fraction(ui) {
                    self.zoom_at(f, 1.0 / c::ZOOM_FACTOR);
                }
            } else if scroll < 0.0
                && let Some(f) = fraction(ui)
            {
                self.zoom_at(f, c::ZOOM_FACTOR);
            }
        }
        // Right-click at a time position creates a timestamp annotation there.
        if response.secondary_clicked()
            && let Some(f) = fraction(ui)
            && let Some(seconds) = self.window().fraction_to_time(f)
        {
            self.add_annotations(
                vec![Annotation {
                    seconds,
                    text: String::new(),
                }],
                true,
            );
            return;
        }
        if (response.clicked()
            || response.drag_stopped()
            || (response.dragged()
                && self.seek_sent.elapsed() > Duration::from_millis(r::SEEK_THROTTLE_MS)))
            && let Some(f) = fraction(ui)
            && let Some(seconds) = self.window().fraction_to_time(f)
        {
            self.seek(seconds);
            if response.clicked() {
                // A plain click on a plot section drops a marker line there and
                // preselects the annotation field with that timestamp, ready to
                // type the note (the line and the field render each other).
                self.add_annotations(
                    vec![Annotation {
                        seconds,
                        text: String::new(),
                    }],
                    false,
                );
                self.annotation_text = annotations::format_annotations(&self.annotations);
                self.focus_annotation_field = true;
            }
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
        let save =
            ctx.input(|i| i.key_pressed(egui::Key::S) && (i.modifiers.command || i.modifiers.ctrl));
        if save && self.annotations_dirty {
            self.request_save_dialog(ctx);
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
        if self.pending_selection.is_some() {
            let mut open = true;
            egui::Window::new("Unsaved annotations")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("The current recording has unsaved annotation changes.");
                    ui.horizontal(|ui| {
                        if ui.button("Save…").clicked() {
                            self.resume_selection = true;
                            self.request_save_dialog(ctx);
                        }
                        if ui.button("Dismiss (lose changes)").clicked() {
                            self.annotations = self.saved_annotations.clone();
                            self.annotations_dirty = false;
                            self.annotation_text =
                                annotations::format_annotations(&self.annotations);
                            self.proceed_pending_selection();
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_selection = None;
                        }
                    });
                });
            if !open {
                self.pending_selection = None; // window X = cancel
            }
        }
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
