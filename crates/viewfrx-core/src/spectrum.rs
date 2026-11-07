//! Single-window magnitude spectrum in amplitude dBFS at a chosen position.
//! Shares the spectrogram's calibrated Hann STFT (coherent-gain corrected,
//! one-sided, channel-power averaged) and its display bounds.
use crate::{
    config::{ANALYSIS_CANCEL_INTERVAL, spectral::*},
    decode::AudioReader,
    spectrogram::{Settings, Spectrogram, Transform},
};
use anyhow::{Result, bail};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct Spectrum {
    pub sample_rate: u32,
    /// Position of the window start in the source, in seconds.
    pub position_seconds: f64,
    /// Source frames actually inside the window (zero-padded at EOF).
    pub frames: u64,
    pub fft_size: usize,
    pub frequency_bins: usize,
    pub hz_per_bin: f32,
    /// Low frequencies first; amplitude dBFS, floored at FLOOR_DBFS.
    pub dbfs: Vec<f32>,
    /// ISO 1/3-octave center frequencies up to Nyquist (the display grid).
    pub third_octave_hz: Vec<f32>,
}

/// Lowest grid frequency of the spectrum display, in Hz.
pub const MIN_GRID_HZ: f32 = 20.0;

/// Aggregate a linear-bin spectrum into 1/3-octave bands (max dBFS per band),
/// returning (center Hz, dBFS) pairs — the log-frequency view data.
pub fn third_octave_bands(spectrum: &Spectrum) -> Vec<(f32, f32)> {
    let nyquist = spectrum.sample_rate as f32 / 2.0;
    spectrum
        .third_octave_hz
        .iter()
        .map(|&center| {
            let lo = center * 2f32.powf(-1.0 / 6.0);
            let hi = center * 2f32.powf(1.0 / 6.0);
            let first = ((lo / spectrum.hz_per_bin) as usize).min(spectrum.dbfs.len());
            let last = (((hi / spectrum.hz_per_bin) as usize + 1)
                .clamp(first + 1, spectrum.dbfs.len().max(1)))
            .min(spectrum.dbfs.len());
            let value = spectrum
                .dbfs
                .get(first..last.max(first + 1).min(spectrum.dbfs.len()))
                .map(|b| b.iter().copied().fold(FLOOR_DBFS, f32::max))
                .unwrap_or(FLOOR_DBFS);
            let _ = nyquist;
            (center, value)
        })
        .collect()
}

/// ISO 266-style 1/3-octave center frequencies (1000 * 2^(k/3)) from
/// MIN_GRID_HZ up to `nyquist_hz`, used as the spectrum grid.
pub fn third_octave_centers(nyquist_hz: f32) -> Vec<f32> {
    let mut centers = Vec::new();
    let mut k = -17i32; // 1000 * 2^(k/3) starts just below 20 Hz
    loop {
        let f = 1000.0 * 2f32.powf(k as f32 / 3.0);
        if f > nyquist_hz.max(MIN_GRID_HZ) {
            break;
        }
        if f >= MIN_GRID_HZ {
            centers.push(f);
        }
        k += 1;
    }
    centers
}

impl Spectrum {
    /// Build the spectrum for the time block at `position_seconds` from an
    /// already-computed spectrogram column — spectrum and spectrogram share
    /// the same STFT data and FFT settings, so no re-decode is needed.
    pub fn from_column(spec: &Spectrogram, position_seconds: f64) -> Spectrum {
        let col_seconds = spec.hop_size as f64 / spec.sample_rate.max(1) as f64;
        let column = if col_seconds > 0.0 && spec.columns > 0 {
            ((position_seconds / col_seconds) as usize).min(spec.columns - 1)
        } else {
            0
        };
        let bins = spec.frequency_bins;
        let start = column * bins;
        let end = (start + bins).min(spec.dbfs.len());
        let dbfs = spec.dbfs.get(start..end).unwrap_or(&[]).to_vec();
        let nyquist = spec.sample_rate as f32 / 2.0;
        Spectrum {
            sample_rate: spec.sample_rate,
            position_seconds,
            frames: spec.frames,
            fft_size: spec.fft_size,
            frequency_bins: bins,
            hz_per_bin: nyquist / bins.max(1) as f32,
            third_octave_hz: third_octave_centers(nyquist),
            dbfs,
        }
    }
}

/// Magnitude spectrum of one Hann-windowed block starting at `position_seconds`
/// (clamped to the recording). Streaming and bounded: only fft_size frames are
/// decoded, never the whole file.
pub fn analyze_at(
    path: &Path,
    settings: Settings,
    position_seconds: f64,
    cancelled: impl Fn() -> bool,
) -> Result<Spectrum> {
    settings.validate()?;
    if !position_seconds.is_finite() || position_seconds < 0.0 {
        bail!("position must be finite and non-negative seconds");
    }
    if cancelled() {
        bail!("analysis cancelled");
    }
    let mut reader = AudioReader::open(path)?;
    let sample_rate = reader.info().sample_rate;
    let channels = reader.info().channels;
    if channels > MAX_CHANNELS {
        bail!("spectrum supports at most {MAX_CHANNELS} channels");
    }
    let duration = reader.info().duration_seconds.unwrap_or(f64::INFINITY);
    let position = position_seconds.min(duration);
    if position > 0.0 {
        reader.seek(position)?;
    }
    let size = settings.fft_size;
    let mut transform = Transform::new(size);
    let mut ring = vec![vec![0.0f32; size]; channels];
    let mut frame = vec![0.0f32; channels];
    let mut frames = 0u64;
    while frames < size as u64 && reader.next_frame(&mut frame)? {
        if frames.is_multiple_of(ANALYSIS_CANCEL_INTERVAL) && cancelled() {
            bail!("analysis cancelled");
        }
        for (channel, sample) in ring.iter_mut().zip(&frame) {
            channel[frames as usize] = *sample;
        }
        frames += 1;
    }
    let dbfs = transform.spectrum(&ring, 0)?;
    let nyquist = sample_rate as f32 / 2.0;
    Ok(Spectrum {
        sample_rate,
        position_seconds: position,
        frames,
        fft_size: size,
        frequency_bins: dbfs.len(),
        hz_per_bin: nyquist / dbfs.len() as f32,
        third_octave_hz: third_octave_centers(nyquist),
        dbfs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(path: &Path, hz: f32, seconds: u32) {
        let rate = 8192;
        let mut w = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 2,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for i in 0..rate * seconds {
            let v = 0.5 * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin();
            w.write_sample(v).unwrap();
            w.write_sample(if hz == 1000.0 { -v } else { v }).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn spectrum_is_calibrated_and_positioned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        tone(&path, 1024.0, 2);
        let s = analyze_at(&path, Settings { fft_size: 1024 }, 0.5, || false).unwrap();
        assert_eq!(s.sample_rate, 8192);
        assert_eq!(s.position_seconds, 0.5);
        assert_eq!(s.frames, 1024);
        let peak = s
            .dbfs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let hz = peak as f32 * s.hz_per_bin;
        assert!((hz - 1024.0).abs() <= s.hz_per_bin, "{hz}");
        // 0.5 amplitude sine -> -6.02 dBFS peak (antiphase channel averaged by power).
        assert!((s.dbfs[peak] + 6.0206).abs() < 0.05, "{}", s.dbfs[peak]);
        // Invalid positions/fft sizes are rejected.
        assert!(analyze_at(&path, Settings { fft_size: 1024 }, -1.0, || false).is_err());
        assert!(analyze_at(&path, Settings { fft_size: 1000 }, 0.0, || false).is_err());
        // Cancellation works.
        assert!(analyze_at(&path, Settings { fft_size: 1024 }, 0.0, || true).is_err());
    }

    #[test]
    fn spectrum_from_spectrogram_column_shares_data_and_calibration() {
        use crate::spectrogram;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        tone(&path, 1024.0, 2);
        let spec = spectrogram::analyze(&path, Settings { fft_size: 1024 }, || false).unwrap();
        // The spectrum at a position equals the spectrogram column there.
        let pos = 1.0;
        let from_col = Spectrum::from_column(&spec, pos);
        assert_eq!(from_col.fft_size, spec.fft_size);
        assert_eq!(from_col.frequency_bins, spec.frequency_bins);
        assert_eq!(from_col.sample_rate, spec.sample_rate);
        // Same peak bin and level as a direct single-window analysis nearby.
        let direct = analyze_at(&path, Settings { fft_size: 1024 }, pos, || false).unwrap();
        let peak_col = from_col
            .dbfs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let peak_dir = direct
            .dbfs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert_eq!(peak_col, peak_dir);
        assert!((from_col.dbfs[peak_col] - direct.dbfs[peak_dir]).abs() < 0.5);
    }

    #[test]
    fn third_octave_bands_peak_at_the_tone_frequency() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        tone(&path, 1000.0, 1);
        let s = analyze_at(&path, Settings { fft_size: 2048 }, 0.0, || false).unwrap();
        let bands = third_octave_bands(&s);
        assert!(!bands.is_empty());
        let (center, db) = bands.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        // 1 kHz tone peaks in the 1 kHz 1/3-octave band at about -6 dBFS.
        assert!((center - 1000.0).abs() < 60.0, "{center}");
        assert!((db + 6.0206).abs() < 0.1, "{db}");
    }

    #[test]
    fn third_octave_centers_cover_iso_bands_up_to_nyquist() {
        let centers = third_octave_centers(8000.0);
        assert!(centers.first().unwrap() >= &MIN_GRID_HZ);
        assert!(centers.contains(&1000.0));
        assert!((centers[centers.len() - 1] - 8000.0).abs() < 500.0);
        assert!(centers.iter().all(|f| *f <= 8000.0 + 1.0));
        // Roughly 10 bands per decade: 20 Hz..8 kHz is ~2.6 decades -> ~26 bands.
        assert!((20..=32).contains(&centers.len()), "{}", centers.len());
        // Spacing is one third of an octave: f[k+3] == 2 * f[k].
        for k in 0..centers.len() - 3 {
            assert!((centers[k + 3] / centers[k] - 2.0).abs() < 0.01);
        }
    }

    #[test]
    fn spectrum_at_eof_is_zero_padded_and_finite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("short.wav");
        let mut w = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..100 {
            w.write_sample(1000i16).unwrap();
        }
        w.finalize().unwrap();
        let s = analyze_at(&path, Settings { fft_size: 512 }, 0.0, || false).unwrap();
        assert_eq!(s.frames, 100);
        assert!(s.dbfs.iter().all(|v| v.is_finite() && *v >= FLOOR_DBFS));
        // Beyond EOF clamps to the end and still produces a finite spectrum.
        let s2 = analyze_at(&path, Settings { fft_size: 512 }, 999.0, || false).unwrap();
        assert!(s2.position_seconds <= 100.0 / 8000.0 + 1e-9);
        assert!(s2.dbfs.iter().all(|v| v.is_finite()));
    }
}
