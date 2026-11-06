use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Output},
};

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_frxview"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn fixture(dir: &Path) {
    let mut w = hound::WavWriter::create(
        dir.join("with spaces.WAV"),
        hound::WavSpec {
            channels: 2,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..8000 {
        w.write_sample(8192i16).unwrap();
        w.write_sample(-8192i16).unwrap();
    }
    w.finalize().unwrap();
}

#[test]
fn import_list_inspect_waveform_and_benchmark_without_audio_device() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let imported = success(run(dir.path(), &["import", "with spaces.WAV"]));
    assert_eq!(imported["roots"].as_array().unwrap().len(), 1);
    let listed = success(run(dir.path(), &["list"]));
    assert!(
        listed["path"]
            .as_str()
            .unwrap()
            .ends_with("with spaces.WAV")
    );
    let info = success(run(dir.path(), &["inspect", "with spaces.WAV"]));
    assert_eq!(info["sample_rate"], 8000);
    assert_eq!(info["channels"], 2);
    assert_eq!(info["duration_seconds"], 1.0);
    let first = success(run(
        dir.path(),
        &["waveform", "with spaces.WAV", "--bins", "8"],
    ));
    let second = success(run(
        dir.path(),
        &["waveform", "with spaces.WAV", "--bins", "8"],
    ));
    assert_eq!(first["cache_hit"], false);
    assert_eq!(second["cache_hit"], false);
    assert!(!dir.path().join(".frxview").exists());
    assert_eq!(first["waveform"], second["waveform"]);
    assert_eq!(first["waveform"]["channel_peaks"][0][0]["min"], 0.25);
    assert_eq!(first["waveform"]["channel_peaks"][1][0]["max"], -0.25);
    assert_eq!(
        first["waveform"]["dc_offset"],
        serde_json::json!([0.25, -0.25])
    );
    let benchmark = success(run(
        dir.path(),
        &[
            "benchmark",
            "with spaces.WAV",
            "--sample-rate",
            "8000",
            "--rate",
            "2",
        ],
    ));
    assert_eq!(benchmark["frames"], 4000);
    // Playback DSP removes the fixture's pure DC offset (L=+0.25, R=-0.25),
    // so only the startup transient energy remains instead of 500.0.
    let dc_energy = benchmark["sum_squares"].as_f64().unwrap();
    assert!(dc_energy < 100.0, "DC offset must be removed: {dc_energy}");
    // The fixture is pure DC (L=+0.25, R=-0.25): it is listed as a file
    // parameter in dBFS and excluded from normalization, which therefore
    // leaves the (now silent) signal at unity gain.
    let levels = success(run(dir.path(), &["levels", "with spaces.WAV"]));
    let lv = &levels["levels"];
    assert_eq!(lv["dc_offset"], serde_json::json!([0.25, -0.25]));
    for db in lv["dc_offset_dbfs"].as_array().unwrap() {
        assert!((db.as_f64().unwrap() + 12.0412).abs() < 0.01, "{db}");
    }
    assert_eq!(lv["sample_peak"], 0.25);
    assert_eq!(lv["dc_removed_peak"], 0.0);
    let norm = success(run(dir.path(), &["normalization", "with spaces.WAV"]));
    assert_eq!(norm["source_peak"], 0.0);
    assert_eq!(norm["gain"], 1.0);
    assert_eq!(norm["target_dbfs"], -1.0);
    // Channel views: grouped stereo overviews plus single-channel selection.
    let stereo = success(run(
        dir.path(),
        &["waveform", "with spaces.WAV", "--stereo", "--bins", "8"],
    ));
    let channels = stereo["channels"].as_array().unwrap();
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0]["channel"], 0);
    assert_eq!(channels[0]["peaks"][0]["min"], 0.25);
    assert_eq!(channels[1]["peaks"][0]["max"], -0.25);
    let left = success(run(
        dir.path(),
        &["waveform", "with spaces.WAV", "--channel", "1", "--bins", "8"],
    ));
    assert_eq!(left["waveform"]["channel"], 1);
    assert!(!run(dir.path(), &["waveform", "with spaces.WAV", "--channel", "2"]).status.success());
    assert!(!run(dir.path(), &["waveform", "with spaces.WAV", "--stereo", "--channel", "0"]).status.success());
    // A real AC signal (1 kHz sine, antiphase stereo) still normalizes to
    // -1 dBFS after DC removal.
    let mut w = hound::WavWriter::create(
        dir.path().join("tone.WAV"),
        hound::WavSpec {
            channels: 2,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for i in 0..8000 {
        let v = (0.25 * (std::f32::consts::TAU * 1000.0 * i as f32 / 8000.0).sin() * 32767.0) as i16;
        w.write_sample(v).unwrap();
        w.write_sample(-v).unwrap();
    }
    w.finalize().unwrap();
    let norm = success(run(dir.path(), &["normalization", "tone.WAV"]));
    let peak = norm["source_peak"].as_f64().unwrap();
    assert!((peak - 0.25).abs() < 1e-3, "{peak}");
    assert_eq!(norm["target_dbfs"], -1.0);
    let actual = 20.0 * (norm["gain"].as_f64().unwrap() * peak).log10();
    assert!((actual + 1.0).abs() < 1e-3);
    let levels = success(run(dir.path(), &["levels", "tone.WAV"]));
    for dc in levels["levels"]["dc_offset"].as_array().unwrap() {
        assert!(dc.as_f64().unwrap().abs() < 1e-4, "{dc}");
    }
    for db in levels["levels"]["dc_offset_dbfs"].as_array().unwrap() {
        // Quantization leaves a negligible DC residual: null or very low dBFS.
        match db.as_f64() {
            Some(v) => assert!(v < -60.0, "{v}"),
            None => {}
        }
    }
    // AC content passes the always-on DC blocker: ~500.0 minus the short
    // onset/offset fade at the file edges.
    let ac = success(run(
        dir.path(),
        &["benchmark", "tone.WAV", "--sample-rate", "8000", "--rate", "2"],
    ));
    assert_eq!(ac["frames"], 4000);
    // 1 kHz sine at amplitude 0.25: RMS energy 4000 * 2 * (0.25^2 / 2) = 250,
    // minus only the short onset/offset fade. The DC blocker must pass it.
    let energy = ac["sum_squares"].as_f64().unwrap();
    assert!((energy - 250.0).abs() < 10.0, "AC must pass DC blocker: {energy}");
    let hp = success(run(
        dir.path(),
        &["benchmark", "tone.WAV", "--sample-rate", "8000", "--rate", "2", "--highpass"],
    ));
    let hp_energy = hp["sum_squares"].as_f64().unwrap();
    assert!(
        (hp_energy - energy).abs() < 10.0,
        "20 Hz highpass must not touch a 1 kHz tone: {hp_energy}"
    );
}

#[cfg(feature = "spectrogram")]
#[test]
fn spectrogram_cli_is_calibrated_cached_and_validates_fft_size() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let first = success(run(
        dir.path(),
        &["spectrogram", "with spaces.WAV", "--fft-size", "512"],
    ));
    let second = success(run(
        dir.path(),
        &["spectrogram", "with spaces.WAV", "--fft-size", "512"],
    ));
    assert_eq!(first["cache_hit"], false);
    assert_eq!(second["cache_hit"], false);
    assert!(!dir.path().join(".frxview").exists());
    assert_eq!(first["spectrogram"], second["spectrogram"]);
    let channels = success(run(
        dir.path(),
        &["spectrogram", "with spaces.WAV", "--stereo"],
    ));
    assert_eq!(channels.as_array().unwrap().len(), 2);
    assert!(
        !run(
            dir.path(),
            &["spectrogram", "with spaces.WAV", "--channel", "2"]
        )
        .status
        .success()
    );
    let spectrum = &first["spectrogram"];
    assert_eq!(spectrum["frames"], 8000);
    assert!(spectrum["columns"].as_u64().unwrap() <= 512);
    // Test DC calibration on every full stationary window. Centered boundary
    // padding introduces a real DC step and therefore spectral transients; it
    // must not be mistaken for another full window of the constant fixture.
    let bins = spectrum["frequency_bins"].as_u64().unwrap() as usize;
    let fft = spectrum["fft_size"].as_u64().unwrap();
    let hop = spectrum["hop_size"].as_u64().unwrap();
    let frames = spectrum["frames"].as_u64().unwrap();
    let mut checked = 0;
    for (column, values) in spectrum["dbfs"]
        .as_array()
        .unwrap()
        .chunks_exact(bins)
        .enumerate()
    {
        let center = column as u64 * hop;
        if center >= fft / 2 && center + fft / 2 <= frames {
            let peak = values
                .iter()
                .map(|v| v.as_f64().unwrap())
                .fold(f64::NEG_INFINITY, f64::max);
            assert!((peak - 20.0 * 0.25f64.log10()).abs() < 0.02);
            checked += 1;
        }
    }
    assert!(checked > 0);
    assert!(
        !run(
            dir.path(),
            &["spectrogram", "with spaces.WAV", "--fft-size", "1000"]
        )
        .status
        .success()
    );
}

#[test]
fn annotate_stores_timestamp_markers_in_file_metadata_idempotently() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let file = "with spaces.WAV";
    // Text-field style: [dd:hh:mm:ss] free text.
    let first = success(run(
        dir.path(),
        &["annotate", file, "[00:00:00:05] thunder starts"],
    ));
    let list = first["annotations"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["seconds"], 5.0);
    assert_eq!(list[0]["timestamp"], "[00:00:00:05]");
    assert_eq!(list[0]["text"], "thunder starts");
    // Right-click style: explicit position; same position twice is idempotent.
    let second = success(run(
        dir.path(),
        &["annotate", file, "--seconds", "42", "clap"],
    ));
    assert_eq!(second["annotations"].as_array().unwrap().len(), 2);
    let again = success(run(
        dir.path(),
        &["annotate", file, "--seconds", "42", "clap"],
    ));
    assert_eq!(again["annotations"].as_array().unwrap().len(), 2);
    // Re-entering the same text line does not duplicate either.
    let repeat = success(run(
        dir.path(),
        &["annotate", file, "[00:00:00:05] thunder starts"],
    ));
    assert_eq!(repeat["annotations"].as_array().unwrap().len(), 2);
    // Listing reads them back from the file metadata.
    let listed = success(run(dir.path(), &["annotations", file]));
    assert_eq!(listed["annotations"], repeat["annotations"]);
    // The file is still a normal audio file: inspect and waveform still work.
    let info = success(run(dir.path(), &["inspect", file]));
    assert_eq!(info["sample_rate"], 8000);
    let wave = success(run(dir.path(), &["waveform", file, "--bins", "8"]));
    assert_eq!(wave["waveform"]["frames"], 8000);
    // Invalid input fails without touching the file.
    assert!(!run(dir.path(), &["annotate", file, "no marker"]).status.success());
    assert!(!run(dir.path(), &["annotate", file, "--seconds", "-3", "x"]).status.success());
    assert!(!run(dir.path(), &["annotations", "missing.wav"]).status.success());
}

#[test]
fn bad_files_and_invalid_arguments_fail_without_panics() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    std::fs::write(dir.path().join("bad.mp3"), b"not audio").unwrap();
    for args in [
        vec!["inspect", "missing.wav"],
        vec!["inspect", "bad.mp3"],
        vec!["waveform", "with spaces.WAV", "--bins", "3"],
        vec!["benchmark", "with spaces.WAV", "--rate", "NaN"],
        vec!["benchmark", "with spaces.WAV", "--sample-rate", "0"],
    ] {
        let output = run(dir.path(), &args);
        assert!(!output.status.success(), "{args:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["ok"], false);
        assert!(!error["error"].as_str().unwrap().is_empty());
    }
}
