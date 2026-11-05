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
