use crate::{catalog::atomic_json, config::*, decode::AudioReader, levels::Normalization};
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
    pub fn sample_peak(&self) -> f32 {
        self.peaks.iter().fold(0.0f32, |peak, bin| {
            peak.max(bin.min.abs()).max(bin.max.abs())
        })
    }
    pub fn normalization(&self) -> Result<Normalization> {
        Normalization::from_peak(self.sample_peak())
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
