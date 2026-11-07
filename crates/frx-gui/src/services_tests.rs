use super::*;
use std::time::Instant;

#[test]
fn library_worker_streams_sorted_deduplicated_results() {
    let dir = tempfile::tempdir().unwrap();
    let sounds = dir.path().join("audio");
    std::fs::create_dir(&sounds).unwrap();
    for name in ["b.wav", "a.wav", "skip.txt"] {
        std::fs::write(sounds.join(name), b"").unwrap();
    }
    let library = LibraryService::start(egui::Context::default(), dir.path().join("catalog.json"));
    let id = library.request(vec![sounds.clone(), sounds.join("a.wav")], true);
    let mut names = Vec::new();
    loop {
        match library.rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            ScanEvent::Batch(generation, entries) => {
                assert_eq!(id, generation);
                names.extend(entries.into_iter().map(|e| e.name));
            }
            ScanEvent::Done(generation, error) => {
                assert_eq!(id, generation);
                assert!(error.is_none(), "{error:?}");
                break;
            }
        }
    }
    assert_eq!(names, ["a.wav", "b.wav"]);
    assert!(!dir.path().join("catalog.json").exists());
    let replacement = dir.path().join("replacement");
    std::fs::create_dir(&replacement).unwrap();
    std::fs::write(replacement.join("c.wav"), b"").unwrap();
    let id = library.request(vec![replacement], true);
    let mut names = Vec::new();
    loop {
        match library.rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            ScanEvent::Batch(generation, entries) if generation == id => {
                names.extend(entries.into_iter().map(|e| e.name))
            }
            ScanEvent::Done(generation, error) if generation == id => {
                assert!(error.is_none());
                break;
            }
            _ => {}
        }
    }
    assert_eq!(names, ["c.wav"]);
}

#[test]
fn analysis_latest_request_wins_and_returns_metadata_before_waveform() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.mp3");
    std::fs::write(
        &path,
        include_bytes!("../../frx-core/tests/fixtures/tone.mp3"),
    )
    .unwrap();
    let service = AnalysisService::start(egui::Context::default(), dir.path().join("cache"));
    service.request(dir.path().join("missing.wav"));
    let id = service.request(path);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut metadata = false;
    while Instant::now() < deadline {
        match service.rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            AnalysisEvent::Metadata(generation, result) if generation == id => {
                assert_eq!(result.unwrap().channels, 2);
                metadata = true;
            }
            AnalysisEvent::Waveform(generation, result) if generation == id => {
                assert!(metadata);
                assert_eq!(result.unwrap().frames, 11025);
                return;
            }
            _ => {} // In-flight older responses are ignored by the UI too.
        }
    }
    panic!("latest analysis did not complete");
}

#[test]
fn full_result_queue_can_be_cancelled_without_a_consumer() {
    let (tx, _rx) = mpsc::sync_channel(1);
    tx.send(1).unwrap();
    assert!(!deliver(&tx, 2, || true, &egui::Context::default()));
}

#[cfg(feature = "spectrogram")]
#[test]
fn spectral_worker_delivers_color_image_after_waveform() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.mp3");
    std::fs::write(
        &path,
        include_bytes!("../../frx-core/tests/fixtures/tone.mp3"),
    )
    .unwrap();
    let service = AnalysisService::start(egui::Context::default(), dir.path().join("cache"));
    let id = service.request_with_options(
        path,
        AnalysisOptions {
            spectral: Some(crate::spectral::RenderSettings {
                analysis: frx_core::spectrogram::Settings::default(),
                floor_db: -100.0,
            }),
        },
    );
    let mut waveform = false;
    loop {
        match service.rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            AnalysisEvent::Waveform(generation, result) if generation == id => {
                result.unwrap();
                waveform = true;
            }
            AnalysisEvent::Spectrogram(generation, result) if generation == id => {
                assert!(waveform);
                let (data, rendered) = result.unwrap();
                assert!(data.columns > 0);
                let image = rendered.unwrap();
                assert!(image.image.width() > 0);
                assert_eq!(
                    image.image.height(),
                    frx_core::config::spectral::MAX_FREQUENCY_BINS
                );
                assert_eq!(image.nyquist_hz, 22050.0);
                break;
            }
            _ => {}
        }
    }
}

#[test]
fn refresh_keeps_the_in_use_device_listed() {
    let previous = vec![
        DeviceInfo {
            index: 0,
            name: "HDMI".into(),
            default_config: Some("48 kHz".into()),
        },
        DeviceInfo {
            index: 1,
            name: "USB DAC".into(),
            default_config: None,
        },
    ];
    // Fresh enumeration lost index 1, but the engine still uses it.
    let fresh = vec![DeviceInfo {
        index: 0,
        name: "HDMI".into(),
        default_config: None,
    }];
    let merged = keep_active_device(fresh, Some(1), &previous);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[1].index, 1);
    assert_eq!(merged[1].name, "USB DAC");
    // No active device or still-listed device: unchanged.
    let fresh2 = vec![DeviceInfo {
        index: 0,
        name: "HDMI".into(),
        default_config: None,
    }];
    assert_eq!(keep_active_device(fresh2.clone(), None, &previous).len(), 1);
    assert_eq!(keep_active_device(fresh2, Some(0), &previous).len(), 1);
}

#[cfg(feature = "spectrogram")]
#[test]
fn spectrum_requests_do_not_starve_full_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.mp3");
    std::fs::write(
        &path,
        include_bytes!("../../frx-core/tests/fixtures/tone.mp3"),
    )
    .unwrap();
    let service = AnalysisService::start(egui::Context::default(), dir.path().join("cache"));
    let full_id = service.request(path.clone());
    // Live spectrum requests queued right after must not replace the full job.
    let _ = service.request_spectrum(path.clone(), 512, 0.0);
    let _ = service.request_spectrum(path.clone(), 512, 0.1);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut metadata = false;
    while Instant::now() < deadline {
        match service.rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            AnalysisEvent::Metadata(id, result) if id == full_id => {
                result.unwrap();
                metadata = true;
            }
            AnalysisEvent::Waveform(id, result) if id == full_id => {
                assert!(metadata, "metadata must arrive before the waveform");
                result.unwrap();
                return; // preview loaded despite spectrum traffic: no regression
            }
            _ => {}
        }
    }
    panic!("full analysis was starved by spectrum requests");
}
