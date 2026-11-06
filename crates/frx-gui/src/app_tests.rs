use super::*;

fn test_app(ctx: &egui::Context, directory: &std::path::Path) -> (App, Receiver<AudioRequest>) {
    let (audio, rx) = AudioService::mock();
    let args = Args {
        paths: vec![],
        library: directory.join("library.json"),
        cache: directory.join("cache"),
        muted: true,
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
        ..Default::default()
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
        ..Default::default()
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
                    vec![frx_core::waveform::Peak { min: -0.5, max: 0.5 }; 2],
                    vec![frx_core::waveform::Peak { min: -0.5, max: 0.5 }; 2],
                ],
                dc_offset: vec![0.0, 0.0],
                ..Default::default()
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
                    seek = seek_at(
                        (position.x - response.rect.left()) / response.rect.width(),
                        60.0,
                    );
                }
            });
        });
    }
    assert!((seek.unwrap() - 30.0).abs() < 0.01);
}

#[test]
fn annotation_text_field_and_right_click_write_file_metadata_idempotently() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.wav");
    {
        let mut w = hound::WavWriter::create(
            &path,
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
    // Wait until the file worker's writes are reflected in the app state.
    fn deliver_file_events(app: &mut App, ctx: &egui::Context, expected: usize) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            app.poll(ctx);
            if app.annotations.len() == expected && !app.annotation_text.is_empty() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("expected {expected} annotations, got {:?}", app.annotations);
    }
    // Text-field entry: "[dd:hh:mm:ss] note" is written to the file metadata.
    app.annotation_text = "[00:00:00:02] hello".into();
    app.submit_annotation_text();
    assert!(app.file_id > 0);
    deliver_file_events(&mut app, &ctx, 1);
    assert_eq!(
        app.annotations,
        vec![Annotation {
            seconds: 2.0,
            text: "hello".into()
        }]
    );
    // The stored annotations render back into the text field.
    assert_eq!(app.annotation_text, "[00:00:00:02] hello");
    // Re-submitting the same line is idempotent.
    app.submit_annotation_text();
    deliver_file_events(&mut app, &ctx, 1);
    assert_eq!(app.annotations.len(), 1);
    // Right-click at 3/4 of the 1 s plot creates a marker there (rounded to
    // whole seconds by the [dd:hh:mm:ss] format).
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
                app.handle_seek(response);
            });
        });
    }
    deliver_file_events(&mut app, &ctx, 2);
    assert_eq!(app.annotations.len(), 2);
    assert!(app
        .annotations
        .contains(&Annotation { seconds: 1.0, text: String::new() }));
    // The file still decodes and carries the markers in its tags.
    assert_eq!(frx_core::annotations::read(&path).unwrap(), app.annotations);
    // Invalid text surfaces an error and writes nothing.
    app.annotation_text = "no marker".into();
    app.submit_annotation_text();
    assert!(app.error.is_some());
}
