#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;

use crate::{
    Args,
    model::{Browser, clock, seek_at},
    services::{
        AnalysisEvent, AnalysisService, AudioRequest, AudioService, AudioSnapshot, LibraryService,
        ScanEvent,
    },
    theme,
};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, pos2, vec2};
use frx_audio::Command;
use frx_core::{decode::AudioInfo, waveform::Waveform};
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
    error: Option<String>,
    gain: f32,
    rate: f64,
    device: Option<usize>,
    picker: Option<Receiver<Vec<PathBuf>>>,
    logo: egui::TextureHandle,
    seek_sent: Instant,
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
        let scan_id = library.request(args.paths, true);
        Self {
            audio,
            snapshot: AudioSnapshot::default(),
            analysis: AnalysisService::start(ctx.clone(), args.cache),
            library,
            browser: Browser::default(),
            scan_id,
            analysis_id: 0,
            scanning: true,
            scroll_to_selection: false,
            analyzing: false,
            recursive: true,
            autoplay: true,
            info: None,
            waveform: None,
            error: None,
            gain: if args.muted { 0.0 } else { 1.0 },
            rate: 1.0,
            device: None,
            picker: None,
            logo,
            seek_sent: Instant::now(),
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

    fn send(&mut self, command: Command) {
        if let Err(error) = self.audio.send(AudioRequest::Transport(command)) {
            self.error = Some(error);
        }
    }
    fn select(&mut self, path: PathBuf, audition: bool) {
        self.analysis_id = self.analysis.request(path.clone());
        self.browser.selected = Some(path.clone());
        self.info = None;
        self.waveform = None;
        self.analyzing = true;
        self.error = None;
        if audition {
            self.send(Command::Play { path });
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
        self.scan_id = self.library.request(paths, self.recursive);
        self.browser.clear();
        self.scanning = true;
        self.error = None;
    }
    fn poll(&mut self) {
        if let Some(snapshot) = self.audio.snapshot() {
            self.snapshot = snapshot;
        }
        // Drain only a bounded batch per UI frame, even during large imports.
        for _ in 0..8 {
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
                    if error.is_some() {
                        self.error = error;
                    }
                }
                _ => {}
            }
        }
        for _ in 0..4 {
            let Ok(event) = self.analysis.rx.try_recv() else {
                break;
            };
            match event {
                AnalysisEvent::Metadata(id, result) if id == self.analysis_id => match result {
                    Ok(info) => self.info = Some(info),
                    Err(error) => {
                        self.error = Some(error);
                        self.analyzing = false;
                    }
                },
                AnalysisEvent::Waveform(id, result) if id == self.analysis_id => {
                    self.analyzing = false;
                    match result {
                        Ok(waveform) => self.waveform = Some(waveform),
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
                self.send(Command::Play { path });
                self.send(Command::Resume);
            }
        }
    }
    fn seek(&mut self, seconds: f64) {
        let Some(path) = self.browser.selected.clone() else {
            return;
        };
        if !self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.path.as_ref() == Some(&path) && s.state != "error")
        {
            self.send(Command::Play { path });
        }
        self.send(Command::Seek { seconds });
        self.seek_sent = Instant::now();
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
                5.0
            } else if i.key_pressed(egui::Key::ArrowLeft) {
                -5.0
            } else {
                0.0
            }
        });
        if delta != 0.0 && self.duration() > 0.0 {
            self.seek((self.position() + delta).clamp(0.0, self.duration()));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.send(Command::Stop);
        }
    }

    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(14))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.image((self.logo.id(), vec2(36.0, 36.0)));
                    ui.label(RichText::new("frxview").size(25.0).strong());
                    ui.label(RichText::new("/  JESDEV").color(theme::AMBER).size(12.0));
                    ui.separator();
                    ui.add_enabled_ui(self.picker.is_none(), |ui| {
                        if ui.button("+ Files").clicked() {
                            self.choose(false, ctx);
                        }
                        if ui.button("+ Folder").clicked() {
                            self.choose(true, ctx);
                        }
                    });
                    if self.picker.is_some() {
                        ui.spinner();
                        ui.small("Waiting for file chooser");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("LOCAL AUDIO · NO UPLOADS")
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    });
                });
            });
    }
    fn footer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            let error = self
                .error
                .as_ref()
                .or(self.snapshot.error.as_ref())
                .or_else(|| self.snapshot.status.as_ref().and_then(|s| s.error.as_ref()));
            if let Some(error) = error {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(Color32::from_rgb(255, 132, 113), error);
                });
                if self.error.is_some() && ui.small_button("Dismiss").clicked() {
                    self.error = None;
                }
            }
            ui.horizontal(|ui| {
                ui.small(
                    RichText::new(
                        "Space: play/pause    Up/Down: browse    Left/Right: seek    Esc: stop",
                    )
                    .color(theme::MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.snapshot.opening {
                        ui.spinner();
                        ui.small("Opening output…");
                    } else {
                        ui.small(
                            self.snapshot
                                .status
                                .as_ref()
                                .map_or("No output", |s| s.state),
                        );
                    }
                });
            });
        });
    }
    fn browser_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("browser")
            .default_width(280.0)
            .min_width(200.0)
            .max_width(500.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.heading("LIBRARY");
                    if self.scanning {
                        ui.spinner();
                    }
                });
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.browser.query)
                            .hint_text("Filter filename or path…")
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    self.browser.filter();
                }
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut self.recursive, "Subfolders").changed() {
                        self.rescan(Vec::new());
                    }
                    if ui.small_button("Refresh").clicked() {
                        self.rescan(Vec::new());
                    }
                });
                ui.checkbox(&mut self.autoplay, "Audition on selection");
                ui.small(format!(
                    "{} shown / {} files",
                    self.browser.visible.len(),
                    self.browser.entries.len()
                ));
                ui.separator();
                if self.browser.entries.is_empty() && !self.scanning {
                    ui.add_space(24.0);
                    ui.label("Drop audio files or folders here.");
                    ui.label(
                        RichText::new("Imported files stay where they are.").color(theme::MUTED),
                    );
                } else if self.browser.visible.is_empty() && !self.scanning {
                    ui.label("No matching files.");
                }
                let mut clicked = None;
                let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
                if self.scroll_to_selection {
                    if let Some(row) = self.browser.visible.iter().position(|&i| {
                        Some(&self.browser.entries[i].path) == self.browser.selected.as_ref()
                    }) {
                        scroll = scroll.vertical_scroll_offset(
                            row.saturating_sub(3) as f32 * (30.0 + ui.spacing().item_spacing.y),
                        );
                    }
                    self.scroll_to_selection = false;
                }
                scroll.show_rows(ui, 30.0, self.browser.visible.len(), |ui, range| {
                    for row in range {
                        let entry = &self.browser.entries[self.browser.visible[row]];
                        let selected = self.browser.selected.as_ref() == Some(&entry.path);
                        let response = ui.add_sized(
                            [ui.available_width(), 30.0],
                            egui::Button::new(&entry.name).selected(selected).truncate(),
                        );
                        let response = response.on_hover_text(entry.path.display().to_string());
                        if response.clicked() {
                            clicked = Some(entry.path.clone());
                        }
                    }
                });
                if let Some(path) = clicked {
                    self.select(path, self.autoplay);
                }
            });
    }
    fn settings(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("settings")
            .default_width(230.0)
            .min_width(210.0)
            .max_width(320.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.heading("PLAYBACK");
                ui.add_space(12.0);
                ui.label("Output device");
                let previous = self.device;
                let label = self
                    .device
                    .and_then(|index| {
                        self.snapshot
                            .devices
                            .iter()
                            .find(|d| d.index == index)
                            .map(|d| d.name.as_str())
                    })
                    .unwrap_or("System default");
                egui::ComboBox::from_id_salt("devices")
                    .selected_text(label)
                    .width(ui.available_width() - 10.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.device, None, "System default");
                        for device in &self.snapshot.devices {
                            ui.selectable_value(&mut self.device, Some(device.index), &device.name)
                                .on_hover_text(
                                    device
                                        .default_config
                                        .as_deref()
                                        .unwrap_or("Unavailable configuration"),
                                );
                        }
                    });
                let retry = ui.button("Retry output").clicked();
                if (self.device != previous || retry)
                    && let Err(e) = self.audio.send(AudioRequest::Device(self.device))
                {
                    self.error = Some(e);
                }
                if ui.small_button("Refresh devices").clicked()
                    && let Err(e) = self.audio.send(AudioRequest::Refresh)
                {
                    self.error = Some(e);
                }
                ui.small(RichText::new("Changing output stops playback.").color(theme::MUTED));
                ui.add_space(16.0);
                ui.separator();
                ui.label("Gain");
                if ui
                    .add(egui::Slider::new(&mut self.gain, 0.0..=2.0).suffix(" ×"))
                    .changed()
                {
                    self.send(Command::Gain { value: self.gain });
                }
                ui.horizontal(|ui| {
                    if ui.button("Mute").clicked() {
                        self.gain = 0.0;
                        self.send(Command::Gain { value: 0.0 });
                    }
                    if ui.button("Unity").clicked() {
                        self.gain = 1.0;
                        self.send(Command::Gain { value: 1.0 });
                    }
                });
                ui.add_space(12.0);
                ui.label("Playback rate");
                let rate_response =
                    ui.add(egui::Slider::new(&mut self.rate, 0.25..=4.0).suffix(" ×"));
                // Submit on release/keyboard changes, not every pixel of a rate drag.
                if rate_response.drag_stopped()
                    || (rate_response.changed() && !rate_response.dragged())
                {
                    self.send(Command::Rate { value: self.rate });
                }
                if ui.small_button("Reset 1×").clicked() {
                    self.rate = 1.0;
                    self.send(Command::Rate { value: 1.0 });
                }
                ui.small(
                    RichText::new("Rate changes pitch. Preview-quality interpolation.")
                        .color(theme::MUTED),
                );
                ui.add_space(18.0);
                ui.separator();
                ui.label("OUTPUT PEAK");
                let peak = self.snapshot.status.as_ref().map_or(0.0, |s| s.peak);
                let db = if peak > 0.0 {
                    20.0 * peak.log10()
                } else {
                    -60.0
                };
                let fraction = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .fill(if peak >= 1.0 {
                            Color32::from_rgb(255, 94, 78)
                        } else {
                            theme::AMBER
                        })
                        .text(if peak > 0.0 {
                            format!("{db:.1} dBFS")
                        } else {
                            "−∞ dBFS".into()
                        }),
                );
                ui.small(
                    RichText::new("Callback peak · not a calibrated VU meter").color(theme::MUTED),
                );
                if let Some(status) = &self.snapshot.status {
                    ui.add_space(8.0);
                    ui.small(format!("Clipped samples: {}", status.clipped_samples));
                    ui.small(format!("Buffer shortages: {}", status.underrun_callbacks))
                        .on_hover_text("Includes seek/switch transition gaps.");
                }
                ui.add_space(16.0);
                ui.separator();
                ui.small(
                    RichText::new(
                        "Spectrogram and spectrum analysis are planned, not enabled in this build.",
                    )
                    .color(theme::MUTED),
                );
            });
    }
    fn center(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(22))
            .show(ctx, |ui| {
                let Some(path) = &self.browser.selected else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.25);
                        ui.image((self.logo.id(), vec2(96.0, 96.0)));
                        ui.add_space(20.0);
                        ui.heading("Find the sound.");
                        ui.label(
                            RichText::new("Import a folder. Browse. Listen.").color(theme::MUTED),
                        );
                        ui.add_space(16.0);
                        if ui.button("Open audio folder").clicked() && self.picker.is_none() {
                            self.choose(true, ctx);
                        }
                    });
                    return;
                };
                ui.label(
                    RichText::new("SELECTED RECORDING")
                        .size(10.0)
                        .color(theme::AMBER),
                );
                ui.heading(path.file_name().unwrap_or_default().to_string_lossy())
                    .on_hover_text(path.display().to_string());
                ui.add(
                    egui::Label::new(
                        RichText::new(path.display().to_string())
                            .size(11.0)
                            .color(theme::MUTED),
                    )
                    .truncate(),
                );
                ui.add_space(12.0);
                if let Some(info) = &self.info {
                    ui.horizontal_wrapped(|ui| {
                        for (label, value) in [
                            ("DURATION", clock(self.duration())),
                            ("SAMPLE RATE", format!("{} Hz", info.sample_rate)),
                            ("CHANNELS", info.channels.to_string()),
                            (
                                "RESOLUTION",
                                info.bits_per_sample
                                    .map_or("Compressed".into(), |v| format!("{v}-bit")),
                            ),
                        ] {
                            egui::Frame::new()
                                .fill(theme::PANEL)
                                .inner_margin(10)
                                .corner_radius(5)
                                .show(ui, |ui| {
                                    ui.label(RichText::new(label).size(10.0).color(theme::MUTED));
                                    ui.label(RichText::new(value).strong());
                                });
                        }
                    });
                    if !info.tags.is_empty() {
                        egui::CollapsingHeader::new(format!(
                            "Embedded metadata ({})",
                            info.tags.len()
                        ))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(100.0)
                                .show(ui, |ui| {
                                    for (key, value) in &info.tags {
                                        ui.label(format!("{key}: {value}"));
                                    }
                                });
                        });
                    }
                } else if self.analyzing {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Reading metadata…");
                    });
                }
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("WAVEFORM").size(11.0).color(theme::MUTED));
                    if self.analyzing {
                        ui.spinner();
                        ui.small("Analyzing in background — playback available");
                    }
                });
                let duration = self.duration();
                let position = self.position();
                let height = (ui.available_height() - 135.0).clamp(100.0, 380.0);
                let response =
                    waveform_view(ui, self.waveform.as_ref(), position, duration, height);
                if (response.clicked()
                    || response.drag_stopped()
                    || (response.dragged() && self.seek_sent.elapsed() > Duration::from_millis(80)))
                    && let Some(pointer) = response.interact_pointer_pos()
                    && let Some(seconds) = seek_at(
                        (pointer.x - response.rect.left()) / response.rect.width(),
                        duration,
                    )
                {
                    self.seek(seconds);
                }
                ui.horizontal(|ui| {
                    ui.monospace(clock(position));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.monospace(clock(duration));
                    });
                });
                ui.add_space(12.0);
                let playing = self
                    .snapshot
                    .status
                    .as_ref()
                    .is_some_and(|s| s.state == "playing" && s.path == self.browser.selected);
                ui.horizontal(|ui| {
                    if ui
                        .button("|<")
                        .on_hover_text("Previous visible file")
                        .clicked()
                    {
                        self.navigate(-1);
                    }
                    if ui
                        .add_sized(
                            [110.0, 38.0],
                            egui::Button::new(
                                RichText::new(if playing { "Pause" } else { "Play" })
                                    .color(Color32::BLACK)
                                    .strong(),
                            )
                            .fill(theme::AMBER),
                        )
                        .clicked()
                    {
                        self.toggle_play();
                    }
                    if ui.button("Stop").clicked() {
                        self.send(Command::Stop);
                    }
                    if ui.button(">|").on_hover_text("Next visible file").clicked() {
                        self.navigate(1);
                    }
                });
                ui.small(RichText::new("Click or drag the waveform to seek.").color(theme::MUTED));
            });
    }

    fn diagnostic(&mut self, ctx: &egui::Context) {
        let Some(seconds) = self.smoke_seconds else {
            return;
        };
        let elapsed = self.started.elapsed().as_secs_f64();
        if elapsed >= seconds && !self.smoke_reported {
            self.smoke_reported = true;
            println!(
                "{}",
                serde_json::json!({
                    "library_candidates": self.browser.entries.len(),
                    "visible_count": self.browser.visible.len(),
                    "scanning": self.scanning, "analyzing": self.analyzing,
                    "waveform_bins": self.waveform.as_ref().map_or(0, |w| w.peaks.len()),
                    "frames": self.frame_count, "max_update_cpu_ms": self.max_update_ms,
                })
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
        if elapsed > seconds + 10.0 {
            crate::SMOKE_FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
            eprintln!("Screenshot timeout");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let started = Instant::now();
        self.poll();
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
        self.diagnostic(ctx);
        self.frame_count += 1;
        self.max_update_ms = self
            .max_update_ms
            .max(started.elapsed().as_secs_f64() * 1000.0);
    }
}

fn waveform_view(
    ui: &mut egui::Ui,
    waveform: Option<&Waveform>,
    position: f64,
    duration: f64,
    height: f32,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, theme::PANEL);
    for i in 1..4 {
        let y = rect.top() + rect.height() * i as f32 / 4.0;
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1.0, theme::RAISED),
        );
    }
    for i in 1..10 {
        let x = rect.left() + rect.width() * i as f32 / 10.0;
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, theme::RAISED),
        );
    }
    if let Some(waveform) = waveform {
        // Group bins per physical column, preserving extrema instead of skipping peaks.
        let columns = (rect.width().ceil() as usize)
            .max(1)
            .min(waveform.peaks.len());
        for column in 0..columns {
            let start = column * waveform.peaks.len() / columns;
            let end = ((column + 1) * waveform.peaks.len() / columns).max(start + 1);
            let (min, max) = waveform.peaks[start..end]
                .iter()
                .fold((1f32, -1f32), |(min, max), p| {
                    (min.min(p.min), max.max(p.max))
                });
            let x = rect.left() + (column as f32 + 0.5) / columns as f32 * rect.width();
            let scale = rect.height() * 0.43;
            painter.line_segment(
                [
                    pos2(x, rect.center().y - max.clamp(-1.0, 1.0) * scale),
                    pos2(x, rect.center().y - min.clamp(-1.0, 1.0) * scale),
                ],
                Stroke::new(1.0, theme::AMBER),
            );
        }
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waveform preview",
            egui::FontId::proportional(14.0),
            theme::MUTED,
        );
    }
    if duration > 0.0 {
        let x = rect.left() + (position / duration).clamp(0.0, 1.0) as f32 * rect.width();
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(2.0, theme::PURPLE),
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
