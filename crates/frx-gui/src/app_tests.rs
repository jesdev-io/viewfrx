use super::*;

fn test_app(ctx: &egui::Context, directory: &std::path::Path) -> (App, Receiver<AudioRequest>) {
    let (audio, rx) = AudioService::mock();
    let args = Args {
        paths: vec![],
        library: directory.join("library.json"),
        cache: directory.join("cache"),
        muted: true,
        debug_console: false,
        #[cfg(feature = "spectrogram")]
        spectrogram: false,
        smoke_seconds: None,
        screenshot: None,
    };
    (App::with_audio(ctx, args, audio), rx)
}

#[test]
fn keyboard_and_seek_use_the_same_transport_commands_as_cli() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let (mut app, rx) = test_app(&ctx, dir.path());
    app.browser.selected = Some(dir.path().join("a.wav"));
    let input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| app.keyboard(ctx));
    assert!(matches!(
        rx.try_recv().unwrap(),
        AudioRequest::Transport(Command::Play { .. })
    ));
    assert!(matches!(
        rx.try_recv().unwrap(),
        AudioRequest::Transport(Command::Resume)
    ));
    app.seek(0.125);
    assert!(matches!(
        rx.try_recv().unwrap(),
        AudioRequest::Transport(Command::Play { .. })
    ));
    assert!(
        matches!(rx.try_recv().unwrap(), AudioRequest::Transport(Command::Seek { seconds }) if seconds == 0.125)
    );
}

#[test]
fn normalized_start_uses_the_same_peak_as_the_ghost_waveform() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let (mut app, rx) = test_app(&ctx, dir.path());
    let path = dir.path().join("a.wav");
    app.browser.selected = Some(path.clone());
    app.normalize = true;
    app.waveform = Some(Waveform {
        sample_rate: 48000,
        frames: 1,
        frames_per_bin: 1,
        peaks: vec![frx_core::waveform::Peak {
            min: -0.25,
            max: 0.1,
        }],
        channel_peaks: vec![vec![frx_core::waveform::Peak {
            min: -0.25,
            max: 0.1,
        }]],
        dc_offset: vec![0.0],
    });
    let preview = app.waveform.as_ref().unwrap().normalization().unwrap();
    app.start_selected();
    match rx.try_recv().unwrap() {
        AudioRequest::Transport(Command::PlayNormalized { path: actual, peak }) => {
            assert_eq!(actual, path);
            assert_eq!(peak, preview.source_peak);
            assert!(
                (frx_core::levels::amplitude_dbfs(peak * preview.gain).unwrap() + 1.0).abs() < 1e-5
            );
        }
        _ => panic!("expected atomic normalized playback, not ordinary play followed by gain"),
    }
}

fn peak_fixture() -> Waveform {
    Waveform {
        sample_rate: 48000,
        frames: 48000,
        frames_per_bin: 48000,
        peaks: vec![frx_core::waveform::Peak {
            min: -0.25,
            max: 0.1,
        }],
        channel_peaks: vec![vec![frx_core::waveform::Peak {
            min: -0.25,
            max: 0.1,
        }]],
        dc_offset: vec![0.0],
    }
}

#[test]
fn normalized_playback_waits_for_analysis_and_preserves_seek() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let (mut app, rx) = test_app(&ctx, dir.path());
    app.browser.selected = Some(dir.path().join("a.wav"));
    app.normalize = true;
    assert!(!app.start_selected());
    assert!(matches!(
        rx.try_recv().unwrap(),
        AudioRequest::Transport(Command::Stop)
    ));
    app.seek(0.25);
    app.finish_pending_play();
    assert!(rx.try_recv().is_err());
    app.waveform = Some(peak_fixture());
    app.finish_pending_play();
    assert!(matches!(
        rx.try_recv().unwrap(),
        AudioRequest::Transport(Command::PlayNormalized { peak: 0.25, .. })
    ));
    assert!(matches!(
        rx.try_recv().unwrap(),
        AudioRequest::Transport(Command::Seek { seconds: 0.25 })
    ));
    assert!(matches!(
        rx.try_recv().unwrap(),
        AudioRequest::Transport(Command::Resume)
    ));
    assert!(!app.pending_play);
}

#[test]
fn pending_play_respects_stop_selection_changes_and_disabling_normalization() {
    for action in ["stop", "select", "disable"] {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let (mut app, rx) = test_app(&ctx, dir.path());
        app.browser.selected = Some(dir.path().join("a.wav"));
        app.normalize = true;
        app.start_selected();
        rx.try_recv().unwrap();
        match action {
            "stop" => {
                app.stop();
                assert!(matches!(
                    rx.try_recv().unwrap(),
                    AudioRequest::Transport(Command::Stop)
                ));
            }
            "select" => app.select(dir.path().join("b.wav"), false),
            _ => {
                app.normalize = false;
                app.normalization_changed();
                assert!(matches!(
                    rx.try_recv().unwrap(),
                    AudioRequest::Transport(Command::Play { .. })
                ));
                assert!(matches!(
                    rx.try_recv().unwrap(),
                    AudioRequest::Transport(Command::Resume)
                ));
            }
        }
        app.waveform = Some(peak_fixture());
        app.finish_pending_play();
        assert!(!app.pending_play);
        assert!(rx.try_recv().is_err(), "{action}");
    }
}

#[test]
fn empty_and_populated_views_render_without_an_audio_device() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let (mut app, _rx) = test_app(&ctx, dir.path());
    for populated in [false, true] {
        if populated {
            app.browser.append(vec![crate::services::Entry::new(
                "field recording.wav".into(),
            )]);
            app.browser.selected = Some("field recording.wav".into());
            app.info = Some(AudioInfo {
                sample_rate: 48000,
                channels: 2,
                frames: Some(48000),
                duration_seconds: Some(1.0),
                bits_per_sample: Some(16),
                tags: vec![],
            });
            app.waveform = Some(Waveform {
                sample_rate: 48000,
                frames: 48000,
                frames_per_bin: 24000,
                peaks: vec![
                    frx_core::waveform::Peak {
                        min: -0.5,
                        max: 0.5
                    };
                    2
                ],
                channel_peaks: vec![
                    vec![
                        frx_core::waveform::Peak {
                            min: -0.5,
                            max: 0.5
                        };
                        2
                    ],
                    vec![
                        frx_core::waveform::Peak {
                            min: -0.5,
                            max: 0.5
                        };
                        2
                    ],
                ],
                dc_offset: vec![0.0, 0.0],
            });
        }
        for size in [vec2(1280.0, 800.0), vec2(900.0, 560.0)] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), size)),
                    ..Default::default()
                },
                |ctx| {
                    app.header(ctx);
                    app.footer(ctx);
                    app.browser_panel(ctx);
                    app.settings(ctx);
                    app.center(ctx);
                },
            );
            assert!(!output.shapes.is_empty());
        }
    }
}

#[test]
fn actual_waveform_click_maps_to_timeline_position() {
    let ctx = egui::Context::default();
    let mut rect = egui::Rect::NOTHING;
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            rect = waveform_view(ui, None, 0.0, 60.0, 120.0).rect;
        });
    });
    let position = rect.center();
    let mut seek = None;
    for pressed in [true, false] {
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = waveform_view(ui, None, 0.0, 60.0, 120.0);
                if response.clicked() {
                    seek = TimeWindow::full(60.0).fraction_to_time(
                        (position.x - response.rect.left()) / response.rect.width(),
                    );
                }
            });
        });
    }
    assert!((seek.unwrap() - 30.0).abs() < 0.01);
}

#[test]
fn annotations_are_floating_until_saved_and_switching_warns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.wav");
    let other = dir.path().join("b.wav");
    for p in [&path, &other] {
        let mut w = hound::WavWriter::create(
            p,
            hound::WavSpec {
                channels: 2,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for i in 0..8000 {
            let v = if i % 2 == 0 { 4096i16 } else { -4096 };
            w.write_sample(v).unwrap();
            w.write_sample(-v).unwrap();
        }
        w.finalize().unwrap();
    }
    let ctx = egui::Context::default();
    let (mut app, _rx) = test_app(&ctx, dir.path());
    app.browser.selected = Some(path.clone());
    app.info = Some(AudioInfo {
        sample_rate: 8000,
        channels: 2,
        frames: Some(8000),
        duration_seconds: Some(1.0),
        bits_per_sample: Some(16),
        tags: vec![],
    });
    // Text-field entry: floating only, nothing written to disk yet.
    app.annotation_text = "[00:00:02:00] hello".into();
    app.submit_annotation_text();
    assert!(app.annotations_dirty);
    assert_eq!(
        app.annotations,
        vec![Annotation {
            seconds: 2.0,
            text: "hello".into()
        }]
    );
    assert_eq!(app.annotation_text, "[00:00:02:00] hello");
    assert_eq!(
        frx_core::annotations::read(&path).unwrap(),
        Vec::new(),
        "floating annotations must not touch the file"
    );
    // Right-click at 3/4 of the 1 s plot adds a position marker (floating).
    let mut rect = egui::Rect::NOTHING;
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            rect = waveform_view(ui, None, 0.0, 1.0, 120.0).rect;
        });
    });
    let position = pos2(rect.left() + rect.width() * 0.75, rect.center().y);
    for pressed in [true, false] {
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = waveform_view(ui, None, 0.0, 1.0, 120.0);
                app.handle_seek(response, ui);
            });
        });
    }
    assert_eq!(app.annotations.len(), 2);
    assert!(app.annotations.contains(&Annotation {
        seconds: 0.75,
        text: String::new()
    }));
    // Re-submitting the same line is idempotent.
    app.annotation_text = "[00:00:02:00] hello".into();
    app.submit_annotation_text();
    assert_eq!(app.annotations.len(), 2);
    // Switching files with unsaved changes asks first instead of selecting.
    app.select(other.clone(), false);
    assert_eq!(app.pending_selection.as_ref().map(|(p, _)| p), Some(&other));
    assert_eq!(app.browser.selected, Some(path.clone()));
    // Dismiss = annotations lost, selection proceeds.
    app.annotations = app.saved_annotations.clone();
    app.annotations_dirty = false;
    app.proceed_pending_selection();
    assert_eq!(app.browser.selected, Some(other.clone()));
    assert!(app.pending_selection.is_none());
    // Select the original again and save explicitly (in place).
    app.select(path.clone(), false);
    app.annotation_text = "[00:00:02:00] hello".into();
    app.submit_annotation_text();
    assert!(app.annotations_dirty);
    app.save_annotations_to(path.clone());
    assert!(app.file_id > 0);
    // Generous deadline: the full workspace suite runs test binaries in parallel.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        app.poll(&ctx);
        if app.annotations.len() == 1 && !app.annotations_dirty {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!app.annotations_dirty);
    assert_eq!(
        frx_core::annotations::read(&path).unwrap(),
        vec![Annotation {
            seconds: 2.0,
            text: "hello".into()
        }]
    );
    // File still decodes normally.
    assert_eq!(frx_core::decode::inspect(&path).unwrap().frames, Some(8000));
    // Invalid text surfaces an error and changes nothing.
    app.annotation_text = "no marker".into();
    app.submit_annotation_text();
    assert!(app.error.is_some());
    assert_eq!(app.annotations.len(), 1);
}

#[test]
fn scroll_wheel_over_waveform_zooms_around_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let (mut app, _rx) = test_app(&ctx, dir.path());
    app.browser.selected = Some(dir.path().join("a.wav"));
    app.info = Some(AudioInfo {
        sample_rate: 8000,
        channels: 2,
        frames: Some(80000),
        duration_seconds: Some(10.0),
        bits_per_sample: Some(16),
        tags: vec![],
    });
    app.waveform = Some(Waveform {
        sample_rate: 8000,
        frames: 80000,
        frames_per_bin: 10000,
        peaks: vec![
            frx_core::waveform::Peak {
                min: -0.5,
                max: 0.5
            };
            8
        ],
        channel_peaks: vec![
            vec![
                frx_core::waveform::Peak {
                    min: -0.5,
                    max: 0.5
                };
                8
            ],
            vec![
                frx_core::waveform::Peak {
                    min: -0.5,
                    max: 0.5
                };
                8
            ],
        ],
        dc_offset: vec![0.0, 0.0],
    });
    // Render once to learn the waveform rect, then scroll over its 3/4 point.
    let mut rect = egui::Rect::NOTHING;
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            rect = waveform_view(ui, app.waveform.as_ref(), 0.0, 10.0, 120.0).rect;
        });
    });
    let pos = pos2(rect.left() + rect.width() * 0.75, rect.center().y);
    let input = egui::RawInput {
        events: vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 40.0),
                modifiers: Default::default(),
            },
        ],
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let response = waveform_view(ui, app.waveform.as_ref(), 0.0, 10.0, 120.0);
            app.handle_seek(response, ui);
        });
    });
    let zoom = app.zoom.expect("scroll wheel must zoom in");
    // Cursor at 75% of a 10 s file: the window centers toward 7.5 s but is
    // clamped at the file end (end == 10), shifting the center a bit left.
    let center = (zoom.start + zoom.end) / 2.0;
    assert!(center > 6.0 && center <= 7.5 + 1e-9, "center {center}");
    assert!((zoom.end - 10.0).abs() < 1e-9, "end {}", zoom.end);
    // One wheel notch is a gentle view-relative scale, not a 2x cut.
    assert!(
        zoom.len() < 10.0 && zoom.len() > 10.0 / 2.0,
        "len {}",
        zoom.len()
    );
    // Scroll down zooms back out to the full view.
    let input = egui::RawInput {
        events: vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -400.0),
                modifiers: Default::default(),
            },
        ],
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let response = waveform_view(ui, app.waveform.as_ref(), 0.0, 10.0, 120.0);
            app.handle_seek(response, ui);
        });
    });
    assert_eq!(app.zoom, None, "zooming out past full resets");
}

#[test]
fn left_click_only_seeks_and_right_click_creates_preselected_marker() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let (mut app, rx) = test_app(&ctx, dir.path());
    app.browser.selected = Some(dir.path().join("a.wav"));
    app.info = Some(AudioInfo {
        sample_rate: 8000,
        channels: 2,
        frames: Some(8000),
        duration_seconds: Some(1.0),
        bits_per_sample: Some(16),
        tags: vec![],
    });
    let mut rect = egui::Rect::NOTHING;
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            rect = waveform_view(ui, None, 0.0, 1.0, 120.0).rect;
        });
    });
    let click = |button: egui::PointerButton,
                 app: &mut App,
                 ctx: &egui::Context,
                 rect: egui::Rect,
                 frac: f32| {
        let position = pos2(rect.left() + rect.width() * frac, rect.center().y);
        for pressed in [true, false] {
            let input = egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = waveform_view(ui, None, 0.0, 1.0, 120.0);
                    app.handle_seek(response, ui);
                });
            });
        }
    };
    // Left click: seeks only, NO annotation.
    click(egui::PointerButton::Primary, &mut app, &ctx, rect, 0.75);
    assert!(app.annotations.is_empty(), "left click must not annotate");
    assert!(matches!(
        rx.try_recv().unwrap(),
        AudioRequest::Transport(Command::Play { .. })
    ));
    assert!(matches!(
        rx.try_recv().unwrap(),
        AudioRequest::Transport(Command::Seek { seconds }) if (seconds - 0.75).abs() < 1e-6
    ));
    assert!(!app.focus_annotation_field);
    // Right click at 3/4: marker line + field preselected with the timestamp.
    click(egui::PointerButton::Secondary, &mut app, &ctx, rect, 0.75);
    assert_eq!(
        app.annotations,
        vec![Annotation {
            seconds: 0.75,
            text: String::new()
        }]
    );
    assert!(app.annotation_text.contains("[00:00:00:75]"));
    assert!(app.focus_annotation_field);
    assert!(app.annotations_dirty);
    // Opening another annotation while the first has no content deletes the
    // redundant empty one: right-click at 1/4 (0.25 s -> 0 s) -> only the new
    // empty marker at 0 s remains.
    click(egui::PointerButton::Secondary, &mut app, &ctx, rect, 0.25);
    assert_eq!(
        app.annotations,
        vec![Annotation {
            seconds: 0.25,
            text: String::new()
        }],
        "the content-less marker at 0.75 s must be deleted when a new one opens"
    );
    // Typing the note attaches it to the marker (empty marker replaced).
    app.focus_annotation_field = false;
    app.annotation_text = format!("{} thunder", app.annotation_text.trim_end());
    app.sync_annotations_from_text();
    assert_eq!(
        app.annotations,
        vec![Annotation {
            seconds: 0.25,
            text: "thunder".into()
        }]
    );
}

#[test]
fn right_click_maps_cursor_position_to_absolute_time() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let (mut app, _rx) = test_app(&ctx, dir.path());
    app.browser.selected = Some(dir.path().join("a.wav"));
    app.info = Some(AudioInfo {
        sample_rate: 8000,
        channels: 2,
        frames: Some(80000),
        duration_seconds: Some(10.0),
        bits_per_sample: Some(16),
        tags: vec![],
    });
    let mut rect = egui::Rect::NOTHING;
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            rect = waveform_view(ui, None, 0.0, 10.0, 120.0).rect;
        });
    });
    // Right-click at 20% / 50% / 80% of a 10 s file -> markers at 2/5/8 s.
    for (frac, expected) in [(0.2f32, 2.0f64), (0.5, 5.0), (0.8, 8.0)] {
        app.annotations.clear();
        let position = pos2(rect.left() + rect.width() * frac, rect.center().y);
        for pressed in [true, false] {
            let input = egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = waveform_view(ui, None, 0.0, 10.0, 120.0);
                    app.handle_seek(response, ui);
                });
            });
        }
        assert_eq!(
            app.annotations,
            vec![Annotation {
                seconds: expected,
                text: String::new()
            }],
            "click at {frac} of 10 s"
        );
    }
}

#[test]
fn typing_annotation_text_never_accumulates_per_keystroke_markers() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let (mut app, _rx) = test_app(&ctx, dir.path());
    app.browser.selected = Some(dir.path().join("a.wav"));
    app.info = Some(AudioInfo {
        sample_rate: 8000,
        channels: 2,
        frames: Some(8000),
        duration_seconds: Some(1.0),
        bits_per_sample: Some(16),
        tags: vec![],
    });
    // Simulate typing "test" onto a marker at 0.88 s, one keystroke at a time.
    for text in [
        "[00:00:00:88] t",
        "[00:00:00:88] te",
        "[00:00:00:88] tes",
        "[00:00:00:88] test",
    ] {
        app.annotation_text = text.into();
        app.sync_annotations_from_text();
        assert_eq!(
            app.annotations.len(),
            1,
            "no per-keystroke duplicates at {text:?}: {:?}",
            app.annotations
        );
    }
    assert_eq!(
        app.annotations,
        vec![Annotation {
            seconds: 0.88,
            text: "test".into()
        }]
    );
}

#[test]
fn deleting_an_annotation_row_removes_its_marker() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let (mut app, _rx) = test_app(&ctx, dir.path());
    app.browser.selected = Some(dir.path().join("a.wav"));
    app.info = Some(AudioInfo {
        sample_rate: 8000,
        channels: 2,
        frames: Some(8000),
        duration_seconds: Some(1.0),
        bits_per_sample: Some(16),
        tags: vec![],
    });
    app.annotations = vec![
        Annotation {
            seconds: 0.25,
            text: "first".into(),
        },
        Annotation {
            seconds: 0.75,
            text: "second".into(),
        },
    ];
    app.annotation_text = "[00:00:00:25] first\n[00:00:00:75] second".into();
    // Delete the second row from the field -> its marker (and plot bar) is gone.
    app.annotation_text = "[00:00:00:25] first".into();
    app.sync_annotations_from_text();
    assert_eq!(
        app.annotations,
        vec![Annotation {
            seconds: 0.25,
            text: "first".into()
        }]
    );
    assert!(app.annotations_dirty);
    // Deleting all rows clears the list (the file stays untouched until saved).
    app.annotation_text = String::new();
    app.sync_annotations_from_text();
    // Empty text does not parse -> markers stay (half-typed input is safe)...
    assert_eq!(app.annotations.len(), 1);
    app.annotation_text = "[00:00:00:00]".into(); // parseable but empty-ish marker
    app.sync_annotations_from_text();
    assert_eq!(app.annotations.len(), 1);
}
