use super::App;
use crate::{
    config::{controls as c, layout as l, runtime as r},
    model::clock,
    services::AudioRequest,
    theme, widgets,
};
use eframe::egui::{self, Color32, RichText, vec2};
use frx_audio::{
    Command,
    config::{MAX_RATE, MIN_RATE},
};
use frx_core::{
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
                    ui.image((self.logo.id(), vec2(l::HEADER_LOGO, l::HEADER_LOGO)));
                    ui.label(RichText::new("frxview").size(l::BRAND_FONT).strong());
                    ui.label(
                        RichText::new("/  JESDEV")
                            .color(theme::AMBER)
                            .size(l::BRAND_SUBFONT),
                    );
                    ui.separator();
                    // Classic top-bar menu exporting the main CLI-equivalent actions.
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
                            self.seek((self.position() + r::KEY_SEEK_SECONDS).min(self.duration()));
                            ui.close();
                        }
                    });
                    ui.menu_button("View", |ui| {
                        if ui
                            .selectable_value(&mut self.wave_channels, widgets::Channels::Both, "Waveform: Both")
                            .clicked()
                        {
                            ui.close();
                        }
                        if ui
                            .selectable_value(&mut self.wave_channels, widgets::Channels::Left, "Waveform: Left")
                            .clicked()
                        {
                            ui.close();
                        }
                        if ui
                            .selectable_value(&mut self.wave_channels, widgets::Channels::Right, "Waveform: Right")
                            .clicked()
                        {
                            ui.close();
                        }
                        #[cfg(feature = "spectrogram")]
                        if ui
                            .checkbox(&mut self.spectral.enabled, "Spectrogram")
                            .clicked()
                        {
                            if let Some(path) = self.browser.selected.clone() {
                                self.request_analysis(path);
                            }
                            ui.close();
                        }
                    });
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
                                .size(l::SMALL_FONT)
                                .color(theme::MUTED),
                        );
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
        ui.heading("PLAYBACK");
        ui.small(RichText::new("Double-click a control to reset.").color(theme::MUTED));
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
        ui.small(RichText::new("Changing output stops playback.").color(theme::MUTED));
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
        if widgets::reset_toggle(ui, &mut self.highpass, false, "Highpass (infra-sound)")
            .changed()
        {
            self.send(Command::Highpass {
                enabled: self.highpass,
            });
        }
        ui.small(
            RichText::new("Fixed highpass removes infra-sound; DC offset is always removed.")
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
        if self.spectral.controls(ui)
            && let Some(path) = self.browser.selected.clone()
        {
            self.request_analysis(path);
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
            crate::logln!("debug console {}", if self.debug_console { "opened" } else { "closed" });
        }
        ui.small(
            RichText::new("Pipes console messages into a debug window.")
                .color(theme::MUTED),
        );
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
                        ui.image((self.logo.id(), vec2(l::EMPTY_LOGO, l::EMPTY_LOGO)));
                        ui.add_space(l::SPACE_SECTION);
                        ui.heading("Find the sound.");
                        ui.label(
                            RichText::new("Import a folder. Browse. Listen.").color(theme::MUTED),
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
                        #[cfg(feature = "spectrogram")]
                        let spectral_chrome = if self.spectral.enabled {
                            l::SPECTRAL_CHROME_HEIGHT
                        } else {
                            0.0
                        };
                        #[cfg(not(feature = "spectrogram"))]
                        let spectral_chrome = 0.0;
                        let total_height = (content_bottom
                            - ui.cursor().top()
                            - l::TRANSPORT_HEIGHT
                            - l::WAVEFORM_CHROME_HEIGHT
                            - spectral_chrome)
                            .clamp(l::PLOT_MIN_HEIGHT, l::PLOT_MAX_HEIGHT);
                        #[cfg(feature = "spectrogram")]
                        let wave_height = if self.spectral.enabled {
                            total_height * (1.0 - l::SPECTRAL_HEIGHT_FRACTION)
                        } else {
                            total_height
                        };
                        #[cfg(not(feature = "spectrogram"))]
                        let wave_height = total_height;
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
                                duration,
                                wave_height / channels.len() as f32,
                                Some(ch),
                                &self.annotations,
                            );
                            self.handle_seek(response);
                        }
                        #[cfg(feature = "spectrogram")]
                        if self.spectral.enabled {
                            self.spectral.channels.control(ui, "Spectrogram", stereo);
                            let channels = self.spectral.channels.indices(stereo);
                            for &ch in channels {
                                let response = self.spectral.paint(
                                    ui,
                                    position,
                                    duration,
                                    total_height * l::SPECTRAL_HEIGHT_FRACTION
                                        / channels.len() as f32,
                                    ch,
                                    &self.annotations,
                                );
                                self.handle_seek(response);
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.monospace(clock(position));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.monospace(clock(duration));
                                },
                            );
                        });
                        ui.add_space(l::SPACE_MEDIUM);
                        let playing = self.snapshot.status.as_ref().is_some_and(|s| {
                            s.state == "playing" && s.path == self.browser.selected
                        });
                        ui.horizontal(|ui| {
                            // Symbol buttons: ⏮ previous, ▶ play, ❚❚ pause, ■ stop, ⏭ next.
                            if ui
                                .button(RichText::new("⏮").size(l::BODY_FONT))
                                .on_hover_text("Previous visible file")
                                .clicked()
                            {
                                self.navigate(-1);
                            }
                            let symbol = if self.pending_play {
                                "✕"
                            } else if playing {
                                "❚❚"
                            } else {
                                "▶"
                            };
                            if ui
                                .add_sized(
                                    l::PLAY_SIZE,
                                    egui::Button::new(
                                        RichText::new(symbol).color(Color32::BLACK).strong(),
                                    )
                                    .fill(theme::AMBER),
                                )
                                .on_hover_text(if self.pending_play {
                                    "Cancel pending audition"
                                } else if playing {
                                    "Pause (two vertical bars)"
                                } else {
                                    "Play (triangle)"
                                })
                                .clicked()
                            {
                                self.toggle_play();
                            }
                            if ui
                                .button(RichText::new("■").size(l::BODY_FONT))
                                .on_hover_text("Stop")
                                .clicked()
                            {
                                self.stop();
                            }
                            if ui
                                .button(RichText::new("⏭").size(l::BODY_FONT))
                                .on_hover_text("Next visible file")
                                .clicked()
                            {
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
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("ANNOTATIONS")
                                    .size(l::MICRO_FONT)
                                    .color(theme::ANNOTATION),
                            );
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.annotation_text)
                                    .hint_text("[dd:hh:mm:ss] free text — Enter adds it to the file's metadata")
                                    .desired_width(f32::INFINITY),
                            );
                            if response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                self.submit_annotation_text();
                            }
                            response.on_hover_text(format!(
                                "{} marker(s) stored in this file. Right-click a plot to mark the playhead position.",
                                self.annotations.len()
                            ));
                        });
                    });
            });
    }
}
