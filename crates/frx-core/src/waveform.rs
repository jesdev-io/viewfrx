use crate::{catalog::atomic_json, decode::AudioReader};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const VERSION: u32 = 1;
const DEFAULT_BUDGET: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Peak {
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Waveform {
    pub sample_rate: u32,
    pub frames: u64,
    pub frames_per_bin: u64,
    /// Peaks across all channels; not an amplitude-cancelling mono mix.
    pub peaks: Vec<Peak>,
}

#[derive(Serialize)]
pub struct CachedWaveform {
    pub cache_hit: bool,
    pub waveform: Waveform,
}

/// Bounded-memory overview: merge adjacent bins as the file grows. No duration
/// estimate needed; the final bin may contain fewer than frames_per_bin frames.
pub fn analyze(path: &Path, max_bins: usize) -> Result<Waveform> {
    if !(2..=65536).contains(&max_bins) || !max_bins.is_power_of_two() {
        bail!("bins must be a power of two between 2 and 65536");
    }
    let mut reader = AudioReader::open(path)?;
    let mut frame = vec![0.0; reader.info().channels];
    let mut result = Waveform {
        sample_rate: reader.info().sample_rate,
        frames: 0,
        frames_per_bin: 1,
        peaks: Vec::with_capacity(max_bins),
    };
    let mut current = Peak {
        min: f32::INFINITY,
        max: f32::NEG_INFINITY,
    };
    let mut count = 0;
    while reader.next_frame(&mut frame)? {
        for &sample in &frame {
            current.min = current.min.min(sample);
            current.max = current.max.max(sample);
        }
        result.frames += 1;
        count += 1;
        if count == result.frames_per_bin {
            result.peaks.push(current);
            current = Peak {
                min: f32::INFINITY,
                max: f32::NEG_INFINITY,
            };
            count = 0;
            if result.peaks.len() == max_bins {
                for i in 0..max_bins / 2 {
                    let a = result.peaks[2 * i];
                    let b = result.peaks[2 * i + 1];
                    result.peaks[i] = Peak {
                        min: a.min.min(b.min),
                        max: a.max.max(b.max),
                    };
                }
                result.peaks.truncate(max_bins / 2);
                result.frames_per_bin *= 2;
            }
        }
    }
    if count > 0 {
        result.peaks.push(current);
    }
    Ok(result)
}

fn read_cached(path: &Path, bins: usize) -> Option<Waveform> {
    use std::io::Read;
    let file = fs::File::open(path).ok()?;
    // Bound the actual read too, even if another process grows the file after stat.
    let mut bytes = Vec::new();
    file.take(8 * 1024 * 1024).read_to_end(&mut bytes).ok()?;
    let waveform: Waveform = serde_json::from_slice(&bytes).ok()?;
    if waveform.peaks.len() > bins || waveform.sample_rate == 0 || waveform.frames_per_bin == 0 {
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
            budget: DEFAULT_BUDGET,
        }
    }

    fn key(path: &Path, bins: usize) -> Result<String> {
        let canonical = path.canonicalize()?;
        let meta = canonical.metadata()?;
        let modified = meta.modified()?.duration_since(UNIX_EPOCH)?.as_nanos();
        let mut hash = blake3::Hasher::new();
        hash.update(canonical.as_os_str().as_encoded_bytes());
        hash.update(&meta.len().to_le_bytes());
        hash.update(&modified.to_le_bytes());
        hash.update(&(bins as u64).to_le_bytes());
        hash.update(&VERSION.to_le_bytes());
        Ok(hash.finalize().to_hex().to_string())
    }

    pub fn get(&self, path: &Path, bins: usize) -> Result<CachedWaveform> {
        let key = Self::key(path, bins)?;
        let file = self.directory.join(format!("frx-waveform-{key}.json"));
        if let Some(waveform) = read_cached(&file, bins) {
            return Ok(CachedWaveform {
                cache_hit: true,
                waveform,
            });
        }
        let waveform = analyze(path, bins)?;
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
        let mut entries = Vec::new();
        let mut total = 0;
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let owned = name
                .to_str()
                .and_then(|s| s.strip_prefix("frx-waveform-"))
                .and_then(|s| s.strip_suffix(".json"))
                .is_some_and(|key| key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()));
            if owned && entry.file_type()?.is_file() {
                let meta = entry.metadata()?;
                total += meta.len();
                entries.push((meta.modified()?, meta.len(), entry.path()));
            }
        }
        entries.sort_by_key(|entry| entry.0);
        for (_, bytes, path) in entries {
            if total <= self.budget {
                break;
            }
            fs::remove_file(path)?;
            total -= bytes;
        }
        Ok(())
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
