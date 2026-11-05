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
    assert_eq!(second["cache_hit"], true);
    assert_eq!(first["waveform"], second["waveform"]);
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
    assert_eq!(benchmark["sum_squares"], 500.0);
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
