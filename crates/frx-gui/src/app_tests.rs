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
