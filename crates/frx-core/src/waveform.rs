use crate::{
    catalog::atomic_json,
    config::*,
    decode::AudioReader,
    levels::{Normalization, amplitude_dbfs},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Peak {
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Waveform {
    pub sample_rate: u32,
    pub frames: u64,
    pub frames_per_bin: u64,
    /// Peaks across all channels; not an amplitude-cancelling mono mix.
    pub peaks: Vec<Peak>,
    pub channel_peaks: Vec<Vec<Peak>>,
    /// Signed arithmetic mean of every decoded channel (linear amplitude).
    pub dc_offset: Vec<f32>,
}

impl Waveform {
    pub fn channels(&self) -> usize {
        self.channel_peaks.len()
    }
    pub fn sample_peak(&self) -> f32 {
        self.peaks.iter().fold(0.0f32, |peak, bin| {
            peak.max(bin.min.abs()).max(bin.max.abs())
        })
    }
    /// Per-channel DC offset in dBFS; zero DC is None (JSON null), not -inf.
    pub fn dc_offset_dbfs(&self) -> Vec<Option<f32>> {
        self.dc_offset
            .iter()
            .map(|&dc| amplitude_dbfs(dc.abs()))
            .collect()
    }
    /// Sample peak of the DC-removed signal. DC offset must not steer gain
    /// scaling, so normalization and playback both use this peak.
    pub fn dc_removed_sample_peak(&self) -> f32 {
        self.channel_peaks
            .iter()
            .zip(&self.dc_offset)
            .map(|(bins, &dc)| {
                bins.iter().fold(0.0f32, |peak, bin| {
                    peak.max((bin.min - dc).abs()).max((bin.max - dc).abs())
                })
            })
            .fold(0.0f32, f32::max)
    }
    pub fn normalization(&self) -> Result<Normalization> {
        Normalization::from_peak(self.dc_removed_sample_peak())
    }
    /// Peaks of one zero-based source channel, for L/R/Both views.
    pub fn channel(&self, index: usize) -> Option<&[Peak]> {
        self.channel_peaks.get(index).map(Vec::as_slice)
    }
}

/// File-level signal parameters: DC offset (linear and dBFS) plus peak and the
/// DC-removed normalization actually applied during playback.
#[derive(Debug, Serialize)]
pub struct Levels {
    pub sample_rate: u32,
    pub frames: u64,
    pub channels: usize,
    pub dc_offset: Vec<f32>,
    pub dc_offset_dbfs: Vec<Option<f32>>,
    pub sample_peak: f32,
    pub sample_peak_dbfs: Option<f32>,
    pub dc_removed_peak: f32,
    pub dc_removed_peak_dbfs: Option<f32>,
    pub normalization: Normalization,
}

impl Waveform {
    pub fn levels(&self) -> Result<Levels> {
        let dc_removed = self.dc_removed_sample_peak();
        Ok(Levels {
            sample_rate: self.sample_rate,
            frames: self.frames,
            channels: self.channels(),
            dc_offset: self.dc_offset.clone(),
            dc_offset_dbfs: self.dc_offset_dbfs(),
            sample_peak: self.sample_peak(),
            sample_peak_dbfs: amplitude_dbfs(self.sample_peak()),
            dc_removed_peak: dc_removed,
            dc_removed_peak_dbfs: amplitude_dbfs(dc_removed),
            normalization: self.normalization()?,
        })
    }
}

/// One channel's overview for `waveform --channel`/`--stereo` views.
#[derive(Debug, Serialize)]
pub struct ChannelWaveform {
    pub sample_rate: u32,
    pub frames: u64,
    pub frames_per_bin: u64,
    pub channel: usize,
    pub peaks: Vec<Peak>,
    pub dc_offset: f32,
    pub dc_offset_dbfs: Option<f32>,
}

impl Waveform {
    pub fn channel_waveform(&self, index: usize) -> Option<ChannelWaveform> {
        let peaks = self.channel(index)?;
        let dc = *self.dc_offset.get(index)?;
        Some(ChannelWaveform {
            sample_rate: self.sample_rate,
            frames: self.frames,
            frames_per_bin: self.frames_per_bin,
            channel: index,
            peaks: peaks.to_vec(),
            dc_offset: dc,
            dc_offset_dbfs: amplitude_dbfs(dc.abs()),
        })
    }
}

#[derive(Serialize)]
pub struct CachedWaveform {
    pub cache_hit: bool,
    pub waveform: Waveform,
}

/// Bounded-memory overview: merge adjacent bins as the file grows. No duration
/// estimate needed; the final bin may contain fewer than frames_per_bin frames.
pub fn analyze(path: &Path, max_bins: usize) -> Result<Waveform> {
    analyze_cancellable(path, max_bins, || false)
}

/// Cooperative cancellation between decoder frames; cannot interrupt OS reads.
pub fn analyze_cancellable(
    path: &Path,
    max_bins: usize,
    cancelled: impl Fn() -> bool,
) -> Result<Waveform> {
    if cancelled() {
        bail!("analysis cancelled");
    }
    if !(MIN_WAVEFORM_BINS..=MAX_WAVEFORM_BINS).contains(&max_bins) || !max_bins.is_power_of_two() {
        bail!("bins must be a power of two between {MIN_WAVEFORM_BINS} and {MAX_WAVEFORM_BINS}");
    }
    let mut reader = AudioReader::open(path)?;
    let mut frame = vec![0.0; reader.info().channels];
    let mut result = Waveform {
        sample_rate: reader.info().sample_rate,
        frames: 0,
        frames_per_bin: 1,
        peaks: Vec::with_capacity(max_bins),
        channel_peaks: vec![Vec::with_capacity(max_bins); frame.len()],
        dc_offset: vec![0.0; frame.len()],
    };
    let mut sums = vec![0.0f64; frame.len()];
    let empty = Peak {
        min: f32::INFINITY,
        max: f32::NEG_INFINITY,
    };
    let mut channels = vec![empty; frame.len()];
    let mut current = Peak {
        min: f32::INFINITY,
        max: f32::NEG_INFINITY,
    };
    let mut count = 0;
    while reader.next_frame(&mut frame)? {
        if result.frames.is_multiple_of(ANALYSIS_CANCEL_INTERVAL) && cancelled() {
            bail!("analysis cancelled");
        }
        for (ch, &sample) in frame.iter().enumerate() {
            sums[ch] += sample as f64;
            channels[ch].min = channels[ch].min.min(sample);
            channels[ch].max = channels[ch].max.max(sample);
            current.min = current.min.min(sample);
            current.max = current.max.max(sample);
        }
        result.frames += 1;
        count += 1;
        if count == result.frames_per_bin {
            result.peaks.push(current);
            for (bins, peak) in result.channel_peaks.iter_mut().zip(&mut channels) {
                bins.push(*peak);
                *peak = empty;
            }
            current = Peak {
                min: f32::INFINITY,
                max: f32::NEG_INFINITY,
            };
            count = 0;
            if result.peaks.len() == max_bins {
                for bins in
                    std::iter::once(&mut result.peaks).chain(result.channel_peaks.iter_mut())
                {
                    for i in 0..max_bins / 2 {
                        let a = bins[2 * i];
                        let b = bins[2 * i + 1];
                        bins[i] = Peak {
                            min: a.min.min(b.min),
                            max: a.max.max(b.max),
                        };
                    }
                    bins.truncate(max_bins / 2);
                }
                result.frames_per_bin *= 2;
            }
        }
    }
    if count > 0 {
        result.peaks.push(current);
        for (bins, peak) in result.channel_peaks.iter_mut().zip(channels) {
            bins.push(peak);
        }
    }
    if result.frames > 0 {
        result.dc_offset = sums
            .into_iter()
            .map(|sum| (sum / result.frames as f64) as f32)
            .collect();
    }
    Ok(result)
}

fn read_cached(path: &Path, bins: usize) -> Option<Waveform> {
    use std::io::Read;
    let file = fs::File::open(path).ok()?;
    // Bound the actual read too, even if another process grows the file after stat.
    let mut bytes = Vec::new();
    file.take(MAX_WAVEFORM_ENTRY_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let waveform: Waveform = serde_json::from_slice(&bytes).ok()?;
    if waveform.channel_peaks.len() != waveform.dc_offset.len()
        || waveform.dc_offset.iter().any(|v| !v.is_finite())
        || waveform.channel_peaks.iter().any(|c| {
            c.len() != waveform.peaks.len()
                || c.iter()
                    .any(|p| !p.min.is_finite() || !p.max.is_finite() || p.min > p.max)
        })
        || waveform.peaks.len() > bins
        || waveform.sample_rate == 0
        || waveform.frames_per_bin == 0
        || waveform
            .peaks
            .iter()
            .any(|p| !p.min.is_finite() || !p.max.is_finite() || p.min > p.max)
    {
        return None;
    }
    Some(waveform)
}

pub struct WaveformCache {
    directory: PathBuf,
    budget: u64,
}

impl WaveformCache {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            budget: WAVEFORM_CACHE_BYTES,
        }
    }

    fn key(path: &Path, bins: usize) -> Result<String> {
        let parameters = [
            &(bins as u64).to_le_bytes()[..],
            &WAVEFORM_CACHE_VERSION.to_le_bytes()[..],
        ]
        .concat();
        crate::cache::key(path, &parameters)
    }

    pub fn get(&self, path: &Path, bins: usize) -> Result<CachedWaveform> {
        self.get_cancellable(path, bins, || false)
    }

    pub fn get_cancellable(
        &self,
        path: &Path,
        bins: usize,
        cancelled: impl Fn() -> bool,
    ) -> Result<CachedWaveform> {
        if cancelled() {
            bail!("analysis cancelled");
        }
        let key = Self::key(path, bins)?;
        let file = self.directory.join(format!("frx-waveform-{key}.json"));
        if let Some(waveform) = read_cached(&file, bins) {
            return Ok(CachedWaveform {
                cache_hit: true,
                waveform,
            });
        }
        let waveform = analyze_cancellable(path, bins, &cancelled)?;
        if cancelled() {
            bail!("analysis cancelled");
        }
        if Self::key(path, bins)? != key {
            bail!("file changed during analysis; retry");
        }
        atomic_json(&file, &waveform)?;
        self.prune()?;
        Ok(CachedWaveform {
            cache_hit: false,
            waveform,
        })
    }

    fn prune(&self) -> Result<()> {
        crate::cache::prune(&self.directory, "frx-waveform-", self.budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(path: &Path, n: usize) -> Result<()> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec)?;
        for i in 0..n {
            writer.write_sample(if i % 2 == 0 { 16384i16 } else { -16384 })?;
        }
        writer.finalize()?;
        Ok(())
    }
    #[test]
    fn bounded_peaks_cache_invalidation_and_corruption_recovery() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a.wav");
        fixture(&path, 1001)?;
        let cache = WaveformCache::new(dir.path().join("cache"));
        let first = cache.get(&path, 8)?;
        assert!(!first.cache_hit);
        assert_eq!(first.waveform.frames, 1001);
        assert!(first.waveform.peaks.len() <= 8);
        assert!(
            first
                .waveform
                .peaks
                .iter()
                .all(|p| p.max == 0.5 && p.min == -0.5)
        );
        assert!(cache.get(&path, 8)?.cache_hit);
        fixture(&path, 2000)?;
        assert!(!cache.get(&path, 8)?.cache_hit);
        let entry = cache.directory.join(format!(
            "frx-waveform-{}.json",
            WaveformCache::key(&path, 8)?
        ));
        fs::write(entry, b"broken")?;
        assert!(!cache.get(&path, 8)?.cache_hit);
        assert!(cache.get(&path, 3).is_err());
        Ok(())
    }
    #[test]
    fn cancellation_stops_analysis_without_writing_cache() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a.wav");
        fixture(&path, 32000)?;
        let calls = std::cell::Cell::new(0);
        let cache = WaveformCache::new(dir.path().join("cache"));
        let result = cache.get_cancellable(&path, 8, || {
            calls.set(calls.get() + 1);
            calls.get() >= 4
        });
        assert!(result.err().unwrap().to_string().contains("cancelled"));
        assert!(!cache.directory.exists());
        assert_eq!(calls.get(), 4);
        Ok(())
    }

    #[test]
    fn cache_budget_is_enforced() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a.wav");
        fixture(&path, 32)?;
        let cache = WaveformCache {
            directory: dir.path().join("cache"),
            budget: 1,
        };
        fs::create_dir_all(&cache.directory)?;
        let unrelated = cache.directory.join("important.json");
        fs::write(&unrelated, b"user-owned data")?;
        cache.get(&path, 8)?;
        assert_eq!(fs::read(&unrelated)?, b"user-owned data");
        assert_eq!(fs::read_dir(cache.directory)?.count(), 1);
        Ok(())
    }
    #[test]
    fn dc_offset_is_reported_in_dbfs_and_excluded_from_normalization() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("dc.wav");
        // Channel 0: constant +0.25 (pure DC). Channel 1: +0.25 DC plus a
        // +/-0.25 square wave, so its DC-removed peak stays 0.25.
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec)?;
        for i in 0..8000 {
            writer.write_sample(8192i16)?;
            writer.write_sample(if i % 2 == 0 { 16384i16 } else { 0 })?;
        }
        writer.finalize()?;
        let wave = analyze(&path, 8)?;
        assert_eq!(wave.channels(), 2);
        assert_eq!(wave.dc_offset, vec![0.25, 0.25]);
        let dc_db = wave.dc_offset_dbfs();
        assert!((dc_db[0].unwrap() + 12.0412).abs() < 0.01, "{:?}", dc_db);
        assert_eq!(wave.sample_peak(), 0.5);
        assert_eq!(wave.dc_removed_sample_peak(), 0.25);
        let levels = wave.levels()?;
        assert!((levels.dc_removed_peak_dbfs.unwrap() + 12.0412).abs() < 0.01);
        assert!((levels.sample_peak_dbfs.unwrap() + 6.0206).abs() < 0.01);
        // Normalization targets the DC-removed peak, not the raw one.
        let n = wave.normalization()?;
        assert_eq!(n.source_peak, 0.25);
        assert!((amplitude_dbfs(n.gain * 0.25).unwrap() + 1.0).abs() < 1e-5);
        // Pure-DC channel removal yields silence: gain stays unity there.
        let pure = {
            let p = dir.path().join("pure.wav");
            let mut w = hound::WavWriter::create(
                &p,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 8000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )?;
            for _ in 0..800 {
                w.write_sample(8192i16)?;
            }
            w.finalize()?;
            analyze(&p, 8)?
        };
        assert_eq!(pure.dc_offset, vec![0.25]);
        assert_eq!(pure.dc_removed_sample_peak(), 0.0);
        assert_eq!(pure.normalization()?.gain, 1.0);
        assert!((pure.dc_offset_dbfs()[0].unwrap() + 12.0412).abs() < 0.01);
        // Channel views expose per-channel peaks and DC.
        let ch1 = wave.channel_waveform(1).unwrap();
        assert_eq!(ch1.channel, 1);
        assert_eq!(ch1.peaks.len(), wave.peaks.len());
        assert_eq!(ch1.dc_offset, 0.25);
        assert!(wave.channel_waveform(2).is_none());
        assert!(wave.channel(5).is_none());
        Ok(())
    }
    #[test]
    fn decode_and_seek_are_sample_accurate_for_pcm() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a.wav");
        fixture(&path, 8000)?;
        let mut reader = AudioReader::open(&path)?;
        assert_eq!(reader.info().duration_seconds, Some(1.0));
        reader.seek(0.5)?;
        let mut frame = [0.0];
        let mut count = 0;
        while reader.next_frame(&mut frame)? {
            count += 1;
        }
        assert_eq!(count, 4000);
        assert!(reader.seek(f64::NAN).is_err());
        assert!(reader.seek(-1.0).is_err());
        fs::write(dir.path().join("bad.wav"), b"not audio")?;
        assert!(AudioReader::open(&dir.path().join("bad.wav")).is_err());
        Ok(())
    }
}
