use super::App;
use crate::{
    config::{controls as c, layout as l, runtime as r},
    model::clock,
    services::AudioRequest,
    theme, widgets,
};
use eframe::egui::{self, RichText};
use viewfrx_audio::{
    Command,
    config::{MAX_RATE, MIN_RATE},
};
use viewfrx_core::{
    config::{NORMALIZE_TARGET_DBFS, RMS_TIME_CONSTANT_SECONDS},
    levels::amplitude_dbfs,
};

impl App {
    pub(super) fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(l::HEADER_PADDING),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Classic desktop menu bar (File / Playback / View) in the
                    // space freed up on the left.
                    egui::MenuBar::new().ui(ui, |ui| {
                        ui.menu_button("File", |ui| {
                            if ui.button("Import files…").clicked() {
                                self.choose(false, ctx);
                                ui.close();
                            }
                            if ui.button("Import folder…").clicked() {
                                self.choose(true, ctx);
                                ui.close();
                            }
                            if ui.button("Refresh").clicked() {
                                self.rescan(Vec::new());
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    self.annotations_dirty && self.browser.selected.is_some(),
                                    egui::Button::new("Save annotations (Ctrl+S)"),
                                )
                                .clicked()
                            {
                                self.request_save_dialog(ctx);
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Quit").clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                        ui.menu_button("Playback", |ui| {
                            if ui.button("Play / Pause (Space)").clicked() {
                                self.toggle_play();
                                ui.close();
                            }
                            if ui.button("Stop (Esc)").clicked() {
                                self.stop();
                                ui.close();
                            }
                            if ui.button("Previous file").clicked() {
                                self.navigate(-1);
                                ui.close();
                            }
                            if ui.button("Next file").clicked() {
                                self.navigate(1);
                                ui.close();
                            }
                            if ui.button("Seek −5 s").clicked() {
                                self.seek((self.position() - r::KEY_SEEK_SECONDS).max(0.0));
                                ui.close();
                            }
                            if ui.button("Seek +5 s").clicked() {
                                self.seek(
                                    (self.position() + r::KEY_SEEK_SECONDS).min(self.duration()),
                                );
                                ui.close();
                            }
                        });
                        ui.menu_button("Help", |ui| {
                            if ui.button("About viewfrx").clicked() {
                                self.about_open = true;
                                ui.close();
                            }
                        });
                        ui.menu_button("View", |ui| {
                            ui.selectable_value(
                                &mut self.wave_channels,
                                widgets::Channels::Both,
                                "Waveform: Both",
                            );
                            ui.selectable_value(
                                &mut self.wave_channels,
                                widgets::Channels::Left,
                                "Waveform: Left",
                            );
                            ui.selectable_value(
                                &mut self.wave_channels,
                                widgets::Channels::Right,
                                "Waveform: Right",
                            );
                            #[cfg(feature = "spectrogram")]
                            {
                                ui.separator();
                                ui.checkbox(&mut self.spectrum_on, "Spectrum");
                                if ui
                                    .checkbox(&mut self.spectral.enabled, "Spectrogram")
                                    .clicked()
                                    && let Some(path) = self.browser.selected.clone()
                                {
                                    self.request_analysis(path);
                                }
                            }
                        });
                    });
                    if self.picker.is_some() {
                        ui.spinner();
                        ui.small("Waiting for file chooser");
                    }
                    // Branding in the right upper corner.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.image((self.logo.id(), self.logo_size(l::HEADER_LOGO)));
                        ui.label(RichText::new("viewfrx").size(l::BRAND_FONT).strong());
                    });
                });
            });
    }
    pub(super) fn footer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            let error = self
                .error
                .as_ref()
                .or(self.snapshot.error.as_ref())
                .or_else(|| self.snapshot.status.as_ref().and_then(|s| s.error.as_ref()));
            if let Some(error) = error {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(theme::ALERT, error);
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
    pub(super) fn browser_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("browser")
            .default_width(l::BROWSER_WIDTH)
            .min_width(l::BROWSER_MIN)
            .max_width(l::BROWSER_MAX)
            .show(ctx, |ui| {
                ui.add_space(l::SPACE_SMALL);
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
                    if widgets::reset_toggle(
                        ui,
                        &mut self.recursive,
                        c::RECURSIVE_DEFAULT,
                        "Subfolders",
                    )
                    .changed()
                    {
                        self.rescan(Vec::new());
                    }
                    if ui.small_button("Refresh").clicked() {
                        self.rescan(Vec::new());
                    }
                });
                widgets::reset_toggle(
                    ui,
                    &mut self.autoplay,
                    c::AUTOPLAY_DEFAULT,
                    "Audition on selection",
                );
                ui.small(format!(
                    "{} shown / {} files",
                    self.browser.visible.len(),
                    self.browser.entries.len()
                ));
                ui.separator();
                if self.browser.entries.is_empty() && !self.scanning {
                    ui.add_space(l::EMPTY_GAP);
                    ui.label("Drop audio files or folders here.");
                    ui.label(
                        RichText::new("Imported files remain as-is during browsing and listening.")
                            .color(theme::MUTED),
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
                            row.saturating_sub(c::SCROLL_CONTEXT_ROWS) as f32
                                * (l::ROW_HEIGHT + ui.spacing().item_spacing.y),
                        );
                    }
                    self.scroll_to_selection = false;
                }
                scroll.show_rows(
                    ui,
                    l::ROW_HEIGHT,
                    self.browser.visible.len(),
                    |ui, range| {
                        for row in range {
                            let entry = &self.browser.entries[self.browser.visible[row]];
                            let selected = self.browser.selected.as_ref() == Some(&entry.path);
                            let response = ui
                                .add_sized(
                                    [ui.available_width(), l::ROW_HEIGHT],
                                    egui::Button::new(&entry.name).selected(selected).truncate(),
                                )
                                .on_hover_text(entry.path.display().to_string());
                            if response.clicked() {
                                clicked = Some(entry.path.clone());
                            }
                        }
                    },
                );
                if let Some(path) = clicked {
                    self.select(path, self.autoplay);
                }
            });
    }
    pub(super) fn settings(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("settings")
            .default_width(l::SETTINGS_WIDTH)
            .min_width(l::SETTINGS_MIN)
            .max_width(l::SETTINGS_MAX)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("settings_scroll")
                    .show(ui, |ui| self.settings_contents(ui));
            });
    }
    fn settings_contents(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = l::SETTINGS_ITEM_GAP;
        ui.add_space(l::SPACE_SMALL);
        ui.heading("PLAYBACK SETTINGS");
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
        let device = egui::ComboBox::from_id_salt("devices")
            .selected_text(label)
            .width(ui.available_width() - l::COMBO_INSET)
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
            })
            .response
            .on_hover_text("Double-click to reset to the system default output.");
        if device.double_clicked() {
            self.device = None;
        }
        let retry = ui
            .horizontal(|ui| {
                let retry = ui.button("Retry output").clicked();
                if ui.small_button("Refresh").clicked()
                    && let Err(e) = self.audio.send(AudioRequest::Refresh)
                {
                    self.error = Some(e);
                }
                retry
            })
            .inner;
        if self.device != previous || retry {
            self.stop();
            if let Err(e) = self.audio.send(AudioRequest::Device(self.device)) {
                self.error = Some(e);
            }
        }
        ui.separator();
        ui.label("Gain · dB");
        if widgets::reset_slider(
            ui,
            &mut self.gain_db,
            c::GAIN_MIN_DB..=c::GAIN_MAX_DB,
            c::GAIN_DEFAULT_DB,
            " dB",
        )
        .changed()
        {
            self.muted = false;
            self.send_gain();
        }
        if widgets::reset_toggle(ui, &mut self.muted, false, "Mute").changed() {
            self.send_gain();
        }
        if widgets::reset_toggle(
            ui,
            &mut self.normalize,
            c::NORMALIZE_DEFAULT,
            &format!("Normalize ({NORMALIZE_TARGET_DBFS:.0} dBFS)"),
        )
        .changed()
        {
            self.normalization_changed();
        }
        if self.normalize && self.waveform.is_none() {
            ui.small("Normalization pending analysis…");
        }
        if let Some(n) = self.waveform.as_ref().and_then(|w| w.normalization().ok()) {
            ui.small(format!("Auto gain: {:+.1} dB", n.gain_db)).on_hover_text(format!("Linked stereo sample-peak normalization. The {NORMALIZE_TARGET_DBFS:.0} dBFS target is before manual gain; silence stays silent. No DC removal or loudness matching."));
        }
        ui.label("Playback rate");
        let rate = widgets::reset_slider(
            ui,
            &mut self.rate,
            MIN_RATE..=MAX_RATE,
            c::RATE_DEFAULT,
            " ×",
        );
        if rate.drag_stopped() || (rate.changed() && !rate.dragged()) {
            self.send(Command::Rate { value: self.rate });
        }
        ui.small(RichText::new("Rate changes pitch.").color(theme::MUTED));
        if widgets::reset_toggle(ui, &mut self.highpass, false, "20 Hz Highpass").changed() {
            self.send(Command::Highpass {
                enabled: self.highpass,
            });
        }
        ui.small(
            RichText::new(
                "Fixed highpass attenuates frequencies below 20 Hz; DC offset is always removed.",
            )
            .color(theme::MUTED),
        );
        ui.separator();
        ui.label("STEREO LEVEL · dBFS");
        widgets::stereo_meter(ui, self.snapshot.status.as_ref());
        ui.small(
            RichText::new(format!(
                "RMS {:.0} ms · peak markers",
                RMS_TIME_CONSTANT_SECONDS * 1000.0
            ))
            .color(theme::MUTED),
        );
        if let Some(status) = &self.snapshot.status {
            ui.small(format!(
                "Clips: {}   Gaps: {}",
                status.clipped_samples, status.underrun_callbacks
            ))
            .on_hover_text(
                "Clipped output samples; buffer shortages include seek/switch transition gaps.",
            );
        }
        ui.separator();
        #[cfg(feature = "spectrogram")]
        {
            if widgets::reset_toggle(
                ui,
                &mut self.spectrum_on,
                c::SPECTRUM_DEFAULT,
                "Spectrum (live)",
            )
            .changed()
            {
                crate::logln!(
                    "spectrum view {}",
                    if self.spectrum_on { "on" } else { "off" }
                );
            }
            ui.small(RichText::new("Magnitude spectrum at the playhead.").color(theme::MUTED));
            if self.spectral.controls(ui)
                && let Some(path) = self.browser.selected.clone()
            {
                self.request_analysis(path);
            }
        }
        #[cfg(not(feature = "spectrogram"))]
        ui.small("Spectrogram requires a build with --features spectrogram.");
        ui.separator();
        ui.label("DEBUG");
        if widgets::reset_toggle(
            ui,
            &mut self.debug_console,
            c::DEBUG_CONSOLE_DEFAULT,
            "Debug console window",
        )
        .changed()
        {
            crate::logln!(
                "debug console {}",
                if self.debug_console {
                    "opened"
                } else {
                    "closed"
                }
            );
        }
        ui.small(RichText::new("Open the program's debug window.").color(theme::MUTED));
    }
    pub(super) fn center(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(l::CENTRAL_PADDING),
            )
            .show(ctx, |ui| {
                let Some(path) = self.browser.selected.clone() else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * l::EMPTY_TOP_FRACTION);
                        ui.image((self.logo.id(), self.logo_size(l::EMPTY_LOGO)));
                        ui.add_space(l::SPACE_SECTION);
                        ui.heading("A lightning fast audio file browser");
                        ui.label(
                            RichText::new("Import audio. Browse. Listen. See.").color(theme::MUTED),
                        );
                        ui.add_space(l::SPACE_LARGE);
                        if ui.button("Open audio folder").clicked() && self.picker.is_none() {
                            self.choose(true, ctx);
                        }
                    });
                    return;
                };
                let content_bottom = ui.max_rect().bottom();
                egui::ScrollArea::vertical()
                    .id_salt("center_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("SELECTED RECORDING")
                                .size(l::MICRO_FONT)
                                .color(theme::AMBER),
                        );
                        ui.heading(path.file_name().unwrap_or_default().to_string_lossy())
                            .on_hover_text(path.display().to_string());
                        ui.add(
                            egui::Label::new(
                                RichText::new(path.display().to_string())
                                    .size(l::SMALL_FONT)
                                    .color(theme::MUTED),
                            )
                            .truncate(),
                        );
                        ui.add_space(l::SPACE_MEDIUM);
                        if let Some(info) = &self.info {
                            let peak = self.waveform.as_ref().map_or("…".into(), |w| {
                                format!(
                                    "{} dBFS",
                                    widgets::db_label(amplitude_dbfs(w.dc_removed_sample_peak()))
                                )
                            });
                            // DC offset is listed as a file parameter in dBFS; it is
                            // removed during playback and excluded from normalization.
                            let dc = self.waveform.as_ref().map_or("…".into(), |w| {
                                let worst = w
                                    .dc_offset_dbfs()
                                    .into_iter()
                                    .flatten()
                                    .fold(None::<f32>, |a, b| {
                                        Some(match a {
                                            Some(a) => a.max(b),
                                            None => b,
                                        })
                                    });
                                widgets::db_label(worst)
                            });
                            ui.horizontal_wrapped(|ui| {
                                for (label, value) in [
                                    ("DURATION", clock(self.duration())),
                                    ("SAMPLE RATE", format!("{} Hz", info.sample_rate)),
                                    ("CHANNELS", info.channels.to_string()),
                                    ("PEAK (DC-REMOVED)", peak),
                                    ("DC OFFSET", format!("{dc} dBFS")),
                                ] {
                                    egui::Frame::new()
                                        .fill(theme::PANEL)
                                        .inner_margin(l::CARD_PADDING)
                                        .corner_radius(l::CARD_RADIUS)
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(label)
                                                    .size(l::MICRO_FONT)
                                                    .color(theme::MUTED),
                                            );
                                            ui.label(RichText::new(value).strong());
                                        });
                                }
                            });
                            ui.small(
                                info.bits_per_sample
                                    .map_or("Compressed audio".into(), |bits| {
                                        format!("{bits}-bit audio")
                                    }),
                            );
                            if !info.tags.is_empty() {
                                egui::CollapsingHeader::new(format!(
                                    "Embedded metadata ({})",
                                    info.tags.len()
                                ))
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .max_height(l::TAGS_MAX_HEIGHT)
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
                        ui.add_space(l::SPACE_MEDIUM);
                        let duration = self.duration();
                        let position = self.position();
                        let window = self.window();
                        #[cfg(feature = "spectrogram")]
                        let spectral_chrome = if self.spectral.enabled {
                            l::SPECTRAL_CHROME_HEIGHT
                        } else {
                            0.0
                        };
                        #[cfg(not(feature = "spectrogram"))]
                        let spectral_chrome = 0.0;
                        #[cfg(feature = "spectrogram")]
                        let bottom_row = if self.spectrum_on {
                            if self.spectrum_height <= 0.0 {
                                self.spectrum_height = l::SPECTRUM_HEIGHT;
                            }
                            self.spectrum_height.clamp(l::PLOT_HEIGHT_MIN, l::PLOT_MAX_HEIGHT)
                                + l::SPACE_SMALL
                        } else {
                            l::ANNOTATION_ROW_DEFAULT + l::SPACE_SMALL
                        };
                        #[cfg(not(feature = "spectrogram"))]
                        let bottom_row = l::ANNOTATION_ROW_DEFAULT + l::SPACE_SMALL;
                        let total_height = (content_bottom
                            - ui.cursor().top()
                            - l::TRANSPORT_HEIGHT
                            - l::WAVEFORM_CHROME_HEIGHT
                            - spectral_chrome
                            - bottom_row
                            - 2.0 * l::SPACE_MEDIUM)
                            .clamp(l::PLOT_MIN_HEIGHT, l::PLOT_MAX_HEIGHT);
                        // User-draggable plot heights (0 = initialize from layout).
                        if self.wave_height <= 0.0 {
                            // Waveforms get less than half the plot area on startup.
                            self.wave_height = (total_height * l::WAVE_HEIGHT_FRACTION)
                                .clamp(l::PLOT_HEIGHT_MIN, l::PLOT_MAX_HEIGHT);
                        }
                        // Initialize only when first shown, matching even a waveform
                        // that was resized before spectrograms were enabled.
                        if spectral_chrome > 0.0 && self.spec_height <= 0.0 {
                            self.spec_height = self.wave_height;
                        }
                        let wave_height = self.wave_height.clamp(l::PLOT_HEIGHT_MIN, l::PLOT_MAX_HEIGHT);
                        #[cfg(feature = "spectrogram")]
                        let spec_height = if self.spectral.enabled {
                            self.spec_height.clamp(l::PLOT_HEIGHT_MIN, l::PLOT_MAX_HEIGHT)
                        } else {
                            0.0
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("ORIGINAL (DC-REMOVED)")
                                    .size(l::SMALL_FONT)
                                    .color(theme::AMBER),
                            )
                            .on_hover_text("Playback removes the DC offset; it is listed as a file parameter in dBFS.");
                            ui.label(
                                RichText::new(format!(
                                    "AUTO-SCALED TO {NORMALIZE_TARGET_DBFS:.0} dBFS"
                                ))
                                .size(l::SMALL_FONT)
                                .color(theme::NORMALIZED_WAVE),
                            );
                            if self.analyzing {
                                ui.spinner();
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Tandem zoom buttons; all plots share the window.
                                    // The +/- buttons zoom into the last play-start
                                    // position (grey line), not the playhead.
                                    let center = window
                                        .time_to_fraction(self.last_play_start.unwrap_or(position));
                                    if ui
                                        .small_button("−")
                                        .on_hover_text("Zoom out (scroll wheel over a plot also zooms)")
                                        .clicked()
                                    {
                                        self.zoom_about_center(center, c::ZOOM_FACTOR);
                                    }
                                    if ui
                                        .small_button("+")
                                        .on_hover_text("Zoom in")
                                        .clicked()
                                    {
                                        self.zoom_about_center(center, 1.0 / c::ZOOM_FACTOR);
                                    }
                                    if self.zoom.is_some() {
                                        ui.small(
                                            RichText::new(format!(
                                                "zoom {:.3}–{:.3} s",
                                                window.start, window.end
                                            ))
                                            .color(theme::MUTED),
                                        );
                                    }
                                },
                            );
                        });
                        let stereo = self.info.as_ref().is_some_and(|i| i.channels == 2);
                        self.wave_channels.control(ui, "Waveform", stereo);
                        let channels = self.wave_channels.indices(stereo);
                        for &ch in channels {
                            if stereo {
                                ui.small(if ch == 0 { "WAVE L" } else { "WAVE R" });
                            }
                            let response = widgets::waveform_channel_view(
                                ui,
                                self.waveform.as_ref(),
                                position,
                                window,
                                wave_height / channels.len() as f32,
                                Some(ch),
                                widgets::PlotOverlays {
                                    annotations: &self.annotations,
                                    play_start: self.last_play_start,
                                },
                            );
                            self.handle_seek(response, ui);
                        }
                        widgets::height_handle(
                            ui,
                            &mut self.wave_height,
                            l::PLOT_HEIGHT_MIN,
                            l::PLOT_MAX_HEIGHT,
                        );
                        #[cfg(feature = "spectrogram")]
                        if self.spectral.enabled {
                            self.spectral.channels.control(ui, "Spectrogram", stereo);
                            let channels = self.spectral.channels.indices(stereo);
                            for &ch in channels {
                                if stereo {
                                    ui.small(if ch == 0 { "SPEC L" } else { "SPEC R" });
                                }
                                let response = self.spectral.paint(
                                    ui,
                                    position,
                                    window,
                                    spec_height / channels.len() as f32,
                                    ch,
                                    widgets::PlotOverlays {
                                        annotations: &self.annotations,
                                        play_start: self.last_play_start,
                                    },
                                );
                                self.handle_seek(response, ui);
                            }
                            widgets::height_handle(
                                ui,
                                &mut self.spec_height,
                                l::PLOT_HEIGHT_MIN,
                                l::PLOT_MAX_HEIGHT,
                            );
                            ui.add_space(l::SPACE_SMALL);
                        }
                        ui.horizontal(|ui| {
                            ui.monospace(clock(position));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if self.zoom.is_some() {
                                        ui.monospace(format!(
                                            "{:.3}–{:.3} s of {}",
                                            window.start,
                                            window.end,
                                            clock(duration)
                                        ));
                                    } else {
                                        ui.monospace(clock(duration));
                                    }
                                },
                            );
                        });
                        ui.add_space(l::SPACE_MEDIUM);
                        ui.horizontal(|ui| {
                            // Painted symbol buttons, all the same size so they
                            // align on one height: ⏮ ▶/❚❚ ■ ⏭.
                            let prev = widgets::transport_button(ui, widgets::TransportSymbol::Prev, false)
                                .on_hover_text("Previous visible file");
                            if prev.clicked() {
                                self.navigate(-1);
                            }
                            let playing = self.snapshot.status.as_ref().is_some_and(|s| {
                                s.state == "playing" && s.path == self.browser.selected
                            });
                            let sym = if self.pending_play {
                                widgets::TransportSymbol::Cancel
                            } else if playing {
                                widgets::TransportSymbol::Pause
                            } else {
                                widgets::TransportSymbol::Play
                            };
                            let play = widgets::transport_button(ui, sym, true).on_hover_text(
                                if self.pending_play {
                                    "Cancel pending audition"
                                } else if playing {
                                    "Pause (two vertical bars)"
                                } else {
                                    "Play (triangle)"
                                },
                            );
                            if play.clicked() {
                                self.toggle_play();
                            }
                            let stop = widgets::transport_button(ui, widgets::TransportSymbol::Stop, false)
                                .on_hover_text("Stop");
                            if stop.clicked() {
                                self.stop();
                            }
                            let next = widgets::transport_button(ui, widgets::TransportSymbol::Next, false)
                                .on_hover_text("Next visible file");
                            if next.clicked() {
                                self.navigate(1);
                            }
                        });
                        ui.small(
                            RichText::new(
                                "Click/drag to seek. Right-click a plot to add a timestamp marker. Double-click a plot to rewind.",
                            )
                            .color(theme::MUTED),
                        );
                        ui.add_space(l::SPACE_SMALL);
                        // Bottom area: two columns — annotations on the left,
                        // spectrum on the right — with a vertical resize bar
                        // between them.
                        // The bottom area autofills down to the window bottom, so
                        // no black gap remains below the annotation/spectrum row.
                        let bottom_h =
                            (content_bottom - ui.cursor().top() - l::SPACE_SMALL)
                                .max(l::PLOT_HEIGHT_MIN);
                        #[cfg(feature = "spectrogram")]
                        if self.spectrum_on {
                            if self.annotation_fraction <= 0.0 {
                                self.annotation_fraction = l::ANNOTATION_FRACTION_DEFAULT;
                            }
                            ui.horizontal(|ui| {
                                let total_w = ui.available_width();
                                let splittable = (total_w - l::WIDTH_HANDLE).max(1.0);
                                let frac = self
                                    .annotation_fraction
                                    .clamp(l::ANNOTATION_FRACTION_MIN, l::ANNOTATION_FRACTION_MAX);
                                let mut ann_w = frac * splittable;
                                let mut spec_w = splittable - ann_w;
                                self.annotation_panel(ui, ann_w, bottom_h);
                                widgets::vsplit_handle(
                                    ui,
                                    &mut ann_w,
                                    &mut spec_w,
                                    splittable * l::ANNOTATION_FRACTION_MIN,
                                    l::SPECTRUM_MIN_WIDTH,
                                    bottom_h,
                                );
                                // Store a fraction, not pixels: the split stays
                                // stable when the window/scrollbar width changes.
                                self.annotation_fraction =
                                    (ann_w / splittable).clamp(l::ANNOTATION_FRACTION_MIN, l::ANNOTATION_FRACTION_MAX);
                                // The spectrum column takes the actual remaining width.
                                let spec_w = ui.available_width().max(l::SPECTRUM_MIN_WIDTH);
                                self.spectrum_panel(ui, spec_w, bottom_h);
                            });
                        } else {
                            let w = ui.available_width();
                            self.annotation_panel(ui, w, bottom_h);
                        }
                        #[cfg(not(feature = "spectrogram"))]
                        {
                            let w = ui.available_width();
                            self.annotation_panel(ui, w, bottom_h);
                        }
                    });
            });
    }

    /// Annotation column/row: the [hh:mm:ss:cc] free-text field.
    pub(super) fn annotation_panel(&mut self, ui: &mut egui::Ui, width: f32, height: f32) {
        egui::Frame::new()
            .fill(theme::PANEL)
            .inner_margin(l::CARD_PADDING)
            .corner_radius(l::CARD_RADIUS)
            .show(ui, |ui| {
                ui.set_width((width - 2.0 * l::CARD_PADDING as f32).max(50.0));
                ui.set_min_height((height - 2.0 * l::CARD_PADDING as f32).max(20.0));
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.label(
                    RichText::new("ANNOTATIONS")
                        .size(l::MICRO_FONT)
                        .color(theme::ANNOTATION),
                );
                // Multiline: each [hh:mm:ss:cc] timestamp gets its own text row,
                // and newlines render as real line breaks (no replacement glyph).
                let rows = ((height - 2.0 * l::CARD_PADDING as f32 - 16.0) / 18.0)
                    .max(1.0) as usize;
                let focus = self.focus_annotation_field;
                let cursor_char = self.annotation_cursor_char;
                let response = ui.add(
                    egui::TextEdit::multiline(&mut self.annotation_text)
                        .hint_text("[hh:mm:ss:cc] manually enter timestamp or right-click timeline")
                        .desired_width(f32::INFINITY)
                        .desired_rows(rows),
                );
                // Automatic: every complete marker in the text becomes a
                // floating annotation immediately (idempotent), without
                // rewriting the field while typing.
                if response.changed() {
                    self.sync_annotations_from_text();
                }
                if focus {
                    ui.memory_mut(|m| m.request_focus(response.id));
                    // Place the cursor at the end of the newly created
                    // marker's row (rows are sorted ascending by time).
                    if let Some(pos) = cursor_char {
                        let mut state = egui::text_edit::TextEditState::default();
                        state.cursor =
                            egui::text::CCursorRange::one(egui::text::CCursor::new(pos)).into();
                        ui.memory_mut(|m| m.data.insert_temp(response.id, state));
                    }
                    self.focus_annotation_field = false;
                    self.annotation_cursor_char = None;
                }
                response.on_hover_text(format!(
                    "{} marker(s) on this file ({}). Right-click a plot to mark the playhead position.",
                    self.annotations.len(),
                    if self.annotations_dirty { "unsaved changes" } else { "saved" }
                ));
                });
            });
    }

    /// Spectrum column: live magnitude spectrum with grid and HPF curve.
    #[cfg(feature = "spectrogram")]
    fn spectrum_panel(&mut self, ui: &mut egui::Ui, width: f32, height: f32) {
        egui::Frame::new()
            .fill(theme::PANEL)
            .inner_margin(l::CARD_PADDING)
            .corner_radius(l::CARD_RADIUS)
            .show(ui, |ui| {
                ui.set_width((width - 2.0 * l::CARD_PADDING as f32).max(50.0));
                let mut title = format!(
                    "SPECTRUM · dBFS @ {:.2} s",
                    self.spectrum.as_ref().map_or(0.0, |(p, _)| *p)
                );
                if self.snapshot.status.as_ref().is_some_and(|s| s.highpass) {
                    title.push_str(" · HPF ON");
                }
                let plot_height = (height - 2.0 * l::CARD_PADDING as f32).max(l::PLOT_HEIGHT_MIN);
                widgets::spectrum_title(ui, &title, plot_height);
                let highpass = self
                    .snapshot
                    .status
                    .as_ref()
                    .filter(|s| s.highpass)
                    .and(self.info.as_ref())
                    .map(|info| (viewfrx_audio::config::HIGHPASS_CUTOFF_HZ, info.sample_rate));
                widgets::spectrum_view(
                    ui,
                    self.spectrum.as_ref().map(|(_, s)| s),
                    self.spectral.floor_db() as f32,
                    plot_height,
                    highpass,
                    self.info.as_ref().map_or(48000, |i| i.sample_rate),
                );
            });
    }
}
