//! Bounded streaming STFT overview. Periodic Hann, 75% overlap, amplitude-dBFS
//! calibration, channel-power averaging (never phase-cancelling downmix).
use crate::{cache, catalog::atomic_json, config::spectral::*, decode::AudioReader};
use anyhow::{Result, bail};
use rustfft::{Fft, FftPlanner, num_complex::Complex32};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub fft_size: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            fft_size: DEFAULT_FFT_SIZE,
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<()> {
        if !FFT_SIZES.contains(&self.fft_size) {
            bail!("FFT size must be one of {FFT_SIZES:?}");
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Spectrogram {
    pub sample_rate: u32,
    pub frames: u64,
    pub fft_size: usize,
    pub hop_size: usize,
    pub windows_per_column: usize,
    pub columns: usize,
    pub frequency_bins: usize,
    /// Time-major; low frequencies first in each column. Values are dBFS of
    /// coherent-gain-corrected one-sided amplitude; channel powers are averaged.
    /// Time/frequency aggregation uses maxima to preserve narrow events.
    pub dbfs: Vec<f32>,
}

struct Transform {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    sum_window: f32,
    scratch: Vec<Complex32>,
    buffer: Vec<Complex32>,
    powers: Vec<f32>,
}
impl Transform {
    fn new(size: usize) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(size);
        let window: Vec<_> = (0..size)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / size as f32).cos())
            .collect();
        let scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
        Self {
            fft,
            sum_window: window.iter().sum(),
            window,
            scratch,
            buffer: vec![Complex32::default(); size],
            powers: vec![0.0; size / 2 + 1],
        }
    }
    fn spectrum(&mut self, ring: &[Vec<f32>], start: usize) -> Result<Vec<f32>> {
        let size = self.buffer.len();
        self.powers.fill(0.0);
        for channel in ring {
            for i in 0..size {
                self.buffer[i] = Complex32::new(channel[(start + i) % size] * self.window[i], 0.0);
            }
            self.fft
                .process_with_scratch(&mut self.buffer, &mut self.scratch);
            for (i, power) in self.powers.iter_mut().enumerate() {
                // DC and Nyquist are not duplicated in a one-sided spectrum.
                let factor = if i == 0 || i == size / 2 { 1.0 } else { 2.0 } / self.sum_window;
                *power += self.buffer[i].norm_sqr() * factor * factor / ring.len() as f32;
            }
        }
        if self.powers.iter().any(|v| !v.is_finite()) {
            bail!("spectral power overflow");
        }
        let bins = MAX_FREQUENCY_BINS.min(self.powers.len());
        Ok((0..bins)
            .map(|i| {
                let from = i * self.powers.len() / bins;
                let to = (i + 1) * self.powers.len() / bins;
                let peak = self.powers[from..to].iter().copied().fold(0.0f32, f32::max);
                if peak > 0.0 {
                    (10.0 * peak.log10()).max(FLOOR_DBFS)
                } else {
                    FLOOR_DBFS
                }
            })
            .collect())
    }
}

struct Overview {
    columns: Vec<Vec<f32>>,
    pending: Vec<f32>,
    count: usize,
    windows_per_column: usize,
}
impl Overview {
    fn new(bins: usize) -> Self {
        Self {
            columns: Vec::with_capacity(MAX_COLUMNS),
            pending: vec![FLOOR_DBFS; bins],
            count: 0,
            windows_per_column: 1,
        }
    }
    fn push(&mut self, spectrum: &[f32]) {
        for (target, source) in self.pending.iter_mut().zip(spectrum) {
            *target = target.max(*source);
        }
        self.count += 1;
        if self.count == self.windows_per_column {
            self.columns.push(self.pending.clone());
            self.pending.fill(FLOOR_DBFS);
            self.count = 0;
            if self.columns.len() == MAX_COLUMNS {
                for i in 0..MAX_COLUMNS / 2 {
                    for j in 0..self.pending.len() {
                        self.columns[i][j] = self.columns[2 * i][j].max(self.columns[2 * i + 1][j]);
                    }
                }
                self.columns.truncate(MAX_COLUMNS / 2);
                self.windows_per_column *= 2;
            }
        }
    }
    fn finish(mut self) -> (Vec<f32>, usize, usize) {
        if self.count > 0 {
            self.columns.push(self.pending);
        }
        let columns = self.columns.len();
        (
            self.columns.into_iter().flatten().collect(),
            columns,
            self.windows_per_column,
        )
    }
}

pub fn analyze(
    path: &Path,
    settings: Settings,
    cancelled: impl Fn() -> bool,
) -> Result<Spectrogram> {
    settings.validate()?;
    if cancelled() {
        bail!("analysis cancelled");
    }
    let mut reader = AudioReader::open(path)?;
    let channels = reader.info().channels;
    if channels > MAX_CHANNELS {
        bail!("spectrogram supports at most {MAX_CHANNELS} channels");
    }
    let size = settings.fft_size;
    let hop = size / HOP_DIVISOR;
    let mut transform = Transform::new(size);
    let bins = MAX_FREQUENCY_BINS.min(size / 2 + 1);
    let mut overview = Overview::new(bins);
    let mut ring = vec![vec![0.0; size]; channels];
    let mut frame = vec![0.0; channels];
    let mut frames = 0u64;
    {
        // Leading/trailing half-window padding centers column zero at t=0,
        // rather than shifting every transient earlier by half an FFT window.
        let mut processed = (size / 2) as u64;
        let mut feed = |frame: &[f32]| -> Result<()> {
            if processed.is_multiple_of(hop as u64) && cancelled() {
                bail!("analysis cancelled");
            }
            let at = (processed % size as u64) as usize;
            for (channel, sample) in ring.iter_mut().zip(frame) {
                channel[at] = *sample;
            }
            processed += 1;
            if processed >= size as u64 && (processed - size as u64).is_multiple_of(hop as u64) {
                overview.push(&transform.spectrum(&ring, (processed % size as u64) as usize)?);
            }
            Ok(())
        };
        while reader.next_frame(&mut frame)? {
            feed(&frame)?;
            frames += 1;
        }
        if frames > 0 {
            frame.fill(0.0);
            let final_window_end = size as u64 + ((frames - 1) / hop as u64) * hop as u64;
            for _ in (frames + (size / 2) as u64)..final_window_end {
                feed(&frame)?;
            }
        }
    }
    if cancelled() {
        bail!("analysis cancelled");
    }
    let (dbfs, columns, windows_per_column) = overview.finish();
    Ok(Spectrogram {
        sample_rate: reader.info().sample_rate,
        frames,
        fft_size: size,
        hop_size: hop,
        windows_per_column,
        columns,
        frequency_bins: bins,
        dbfs,
    })
}

#[derive(Debug, Serialize)]
pub struct CachedSpectrogram {
    pub cache_hit: bool,
    pub spectrogram: Spectrogram,
}
pub struct SpectrogramCache {
    directory: PathBuf,
}
impl SpectrogramCache {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }
    pub fn get(
        &self,
        path: &Path,
        settings: Settings,
        cancelled: impl Fn() -> bool,
    ) -> Result<CachedSpectrogram> {
        settings.validate()?;
        let parameters = serde_json::to_vec(&(CACHE_VERSION, settings))?;
        let key = cache::key(path, &parameters)?;
        let entry = self.directory.join(format!("frx-spectrogram-{key}.json"));
        if cancelled() {
            bail!("analysis cancelled");
        }
        if let Some(spectrogram) = read_cached(&entry, settings) {
            return Ok(CachedSpectrogram {
                cache_hit: true,
                spectrogram,
            });
        }
        let spectrogram = analyze(path, settings, &cancelled)?;
        if cancelled() {
            bail!("analysis cancelled");
        }
        if cache::key(path, &parameters)? != key {
            bail!("file changed during spectral analysis; retry");
        }
        atomic_json(&entry, &spectrogram)?;
        cache::prune(&self.directory, "frx-spectrogram-", CACHE_BYTES)?;
        Ok(CachedSpectrogram {
            cache_hit: false,
            spectrogram,
        })
    }
}
fn read_cached(path: &Path, settings: Settings) -> Option<Spectrogram> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_ENTRY_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let s: Spectrogram = serde_json::from_slice(&bytes).ok()?;
    (s.sample_rate > 0
        && s.fft_size == settings.fft_size
        && s.hop_size == settings.fft_size / HOP_DIVISOR
        && s.windows_per_column.is_power_of_two()
        && s.columns <= MAX_COLUMNS
        && s.frequency_bins == MAX_FREQUENCY_BINS.min(settings.fft_size / 2 + 1)
        && s.dbfs.len() == s.columns * s.frequency_bins
        && s.dbfs.iter().all(|v| v.is_finite()))
    .then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tone(path: &Path, seconds: u32, antiphase: bool) {
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
            let v = 0.5 * (std::f32::consts::TAU * 1024.0 * i as f32 / rate as f32).sin();
            w.write_sample(v).unwrap();
            w.write_sample(if antiphase { -v } else { v }).unwrap();
        }
        w.finalize().unwrap();
    }
    #[test]
    fn transient_column_is_centered_at_its_source_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("impulse.wav");
        let rate = 8192;
        let mut w = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for i in 0..rate {
            w.write_sample(if i == rate / 2 { 0.5f32 } else { 0.0 })
                .unwrap();
        }
        w.finalize().unwrap();
        let s = analyze(&path, Settings { fft_size: 512 }, || false).unwrap();
        let strongest = s
            .dbfs
            .chunks_exact(s.frequency_bins)
            .map(|column| column.iter().copied().fold(FLOOR_DBFS, f32::max))
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap()
            .0;
        assert_eq!(strongest * s.hop_size, rate as usize / 2);
    }

    #[test]
    fn calibrated_tone_survives_antiphase_and_overview_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        tone(&path, 20, true);
        let s = analyze(&path, Settings { fft_size: 512 }, || false).unwrap();
        assert!(s.columns <= MAX_COLUMNS);
        assert!(s.windows_per_column > 1);
        let strongest = s.dbfs.iter().copied().fold(FLOOR_DBFS, f32::max);
        assert!((strongest - (-6.0206)).abs() < 0.02, "{strongest}");
        let first = &s.dbfs[..s.frequency_bins];
        let peak_bin = first
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let hz = peak_bin as f32 / s.frequency_bins as f32 * s.sample_rate as f32 / 2.0;
        assert!((hz - 1024.0).abs() <= 32.0, "{hz}");
    }
    #[test]
    fn cancellation_mid_decode_does_not_persist_partial_spectrogram() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        tone(&path, 2, false);
        let cache = SpectrogramCache::new(dir.path().join("cache"));
        let checks = std::cell::Cell::new(0);
        let result = cache.get(&path, Settings::default(), || {
            checks.set(checks.get() + 1);
            checks.get() >= 8
        });
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert!(!cache.directory.exists());
    }

    #[test]
    fn short_silence_is_finite_and_zero_padded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("silence.wav");
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..100 {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
        let spectrum = analyze(&path, Settings::default(), || false).unwrap();
        assert_eq!(spectrum.frames, 100);
        assert_eq!(spectrum.columns, 1);
        assert!(spectrum.dbfs.iter().all(|&v| v == FLOOR_DBFS));
    }

    #[test]
    fn caching_settings_invalidation_and_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        tone(&path, 1, false);
        let cache = SpectrogramCache::new(dir.path().join("cache"));
        let settings = Settings::default();
        assert!(!cache.get(&path, settings, || false).unwrap().cache_hit);
        assert!(cache.get(&path, settings, || false).unwrap().cache_hit);
        assert!(
            !cache
                .get(&path, Settings { fft_size: 512 }, || false)
                .unwrap()
                .cache_hit
        );
        assert!(cache.get(&path, settings, || true).is_err());
        assert!(analyze(&path, Settings { fft_size: 1000 }, || false).is_err());
        tone(&path, 2, false);
        assert!(!cache.get(&path, settings, || false).unwrap().cache_hit);
    }
}
