//! Deterministic preview renderer, also used by the realtime producer.
use crate::config::{MAX_MANUAL_GAIN, MAX_RATE, MIN_RATE};
use anyhow::{Result, bail};
use frx_core::decode::AudioReader;
use std::path::Path;

pub struct Renderer {
    reader: AudioReader,
    channels: usize,
    output_rate: u32,
    rate: f64,
    phase: f64,
    a: [f32; 2],
    b: [f32; 2],
    have_a: bool,
    have_b: bool,
    position: f64,
}

pub fn validate_rate(rate: f64) -> Result<()> {
    if !rate.is_finite() || !(MIN_RATE..=MAX_RATE).contains(&rate) {
        bail!("rate must be between {MIN_RATE} and {MAX_RATE} (changes pitch)");
    }
    Ok(())
}

pub fn validate_gain(gain: f32) -> Result<()> {
    if !gain.is_finite() || !(0.0..=MAX_MANUAL_GAIN).contains(&gain) {
        bail!("linear gain must be between 0 and {MAX_MANUAL_GAIN}");
    }
    Ok(())
}

impl Renderer {
    pub fn open(path: &Path, output_rate: u32, rate: f64) -> Result<Self> {
        validate_rate(rate)?;
        if output_rate == 0 {
            bail!("output sample rate must be nonzero");
        }
        let reader = AudioReader::open(path)?;
        let channels = reader.info().channels;
        if channels > 2 {
            bail!("preview supports mono/stereo; explicit multichannel downmix is not implemented");
        }
        let mut this = Self {
            reader,
            channels,
            output_rate,
            rate,
            phase: 0.0,
            a: [0.0; 2],
            b: [0.0; 2],
            have_a: false,
            have_b: false,
            position: 0.0,
        };
        this.prime()?;
        Ok(this)
    }

    fn prime(&mut self) -> Result<()> {
        self.have_a = self.reader.next_frame(&mut self.a[..self.channels])?;
        self.have_b = self.reader.next_frame(&mut self.b[..self.channels])?;
        if self.channels == 1 {
            self.a[1] = self.a[0];
            self.b[1] = self.b[0];
        }
        self.phase = 0.0;
        Ok(())
    }

    pub fn seek(&mut self, seconds: f64) -> Result<()> {
        self.reader.seek(seconds)?;
        self.position = self
            .reader
            .info()
            .duration_seconds
            .map_or(seconds, |d| seconds.min(d));
        self.prime()
    }

    pub fn set_rate(&mut self, rate: f64) -> Result<()> {
        validate_rate(rate)?;
        self.rate = rate;
        Ok(())
    }
    pub fn position(&self) -> f64 {
        self.position
    }

    /// Linear interpolation for preview, NOT bandlimited conversion. Returns a
    /// stereo frame, with mono duplicated, at the configured output sample rate.
    pub fn next_frame(&mut self) -> Result<Option<[f32; 2]>> {
        if !self.have_a {
            return Ok(None);
        }
        let b = if self.have_b { self.b } else { self.a };
        let sample = std::array::from_fn(|i| self.a[i] + (b[i] - self.a[i]) * self.phase as f32);
        let step = self.reader.info().sample_rate as f64 * self.rate / self.output_rate as f64;
        self.phase += step;
        self.position += self.rate / self.output_rate as f64;
        while self.phase >= 1.0 && self.have_a {
            self.phase -= 1.0;
            self.a = self.b;
            self.have_a = self.have_b;
            self.have_b = self.reader.next_frame(&mut self.b[..self.channels])?;
            if self.channels == 1 {
                self.b[1] = self.b[0];
            }
        }
        Ok(Some(sample))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(path: &Path) -> Result<()> {
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for i in 0..8000 {
            writer.write_sample((i % 100) as i16 * 100)?;
        }
        writer.finalize()?;
        Ok(())
    }
    #[test]
    fn rates_change_duration_and_mono_is_duplicated() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tone.wav");
        fixture(&path)?;
        for (rate, expected) in [(0.5, 16000), (1.0, 8000), (2.0, 4000), (4.0, 2000)] {
            let mut renderer = Renderer::open(&path, 8000, rate)?;
            let mut count = 0;
            while let Some(frame) = renderer.next_frame()? {
                assert_eq!(frame[0], frame[1]);
                count += 1;
            }
            assert_eq!(count, expected, "rate={rate}");
        }
        Ok(())
    }
    #[test]
    fn seek_matches_reference_samples() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tone.wav");
        fixture(&path)?;
        let mut renderer = Renderer::open(&path, 8000, 1.0)?;
        let mut reference = Vec::new();
        while let Some(frame) = renderer.next_frame()? {
            reference.push(frame);
        }
        renderer.seek(0.234)?;
        assert_eq!(renderer.next_frame()?.unwrap(), reference[1872]);
        renderer.seek(0.0)?;
        assert_eq!(renderer.next_frame()?.unwrap(), reference[0]);
        assert!(validate_gain(f32::NAN).is_err());
        assert!(validate_rate(0.0).is_err());
        assert!(validate_rate(f64::INFINITY).is_err());
        Ok(())
    }
}
