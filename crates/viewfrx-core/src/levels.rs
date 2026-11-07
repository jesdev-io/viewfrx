//! Shared amplitude units, linked-channel sample-peak normalization and metering.
use crate::config;
use anyhow::{Result, bail};
use serde::Serialize;

pub fn db_to_gain(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}
/// Silence is None (JSON null), not a non-finite JSON number.
pub fn amplitude_dbfs(amplitude: f32) -> Option<f32> {
    (amplitude.is_finite() && amplitude > 0.0).then(|| 20.0 * amplitude.log10())
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Normalization {
    pub source_peak: f32,
    pub source_peak_dbfs: Option<f32>,
    pub target_dbfs: f32,
    pub gain: f32,
    pub gain_db: f32,
}
impl Normalization {
    pub fn from_peak(peak: f32) -> Result<Self> {
        if !peak.is_finite() || peak < 0.0 {
            bail!("sample peak must be finite and non-negative");
        }
        let gain = if peak == 0.0 {
            1.0
        } else {
            db_to_gain(config::NORMALIZE_TARGET_DBFS) / peak
        };
        if !gain.is_finite() {
            bail!("normalization gain exceeds floating-point range");
        }
        Ok(Self {
            source_peak: peak,
            source_peak_dbfs: amplitude_dbfs(peak),
            target_dbfs: config::NORMALIZE_TARGET_DBFS,
            gain,
            gain_db: amplitude_dbfs(gain).unwrap_or(0.0),
        })
    }
}

/// VU-style stereo RMS envelope, with a documented exponential time constant;
/// NOT an IEC-calibrated mechanical VU meter. Peak markers decay in dB/second.
/// Construct off the callback; push is allocation-free and sample-rate aware.
pub struct StereoMeter {
    rms_power: [f32; 2],
    peak: [f32; 2],
    rms_coefficient: f32,
    peak_coefficient: f32,
}
impl StereoMeter {
    pub fn new(sample_rate: u32) -> Self {
        let sample_rate = sample_rate.max(1) as f32;
        Self {
            rms_power: [0.0; 2],
            peak: [0.0; 2],
            rms_coefficient: (-1.0 / (sample_rate * config::RMS_TIME_CONSTANT_SECONDS)).exp(),
            peak_coefficient: db_to_gain(-config::PEAK_RELEASE_DB_PER_SECOND / sample_rate),
        }
    }
    pub fn push(&mut self, samples: [f32; 2]) {
        for (channel, sample) in samples.into_iter().enumerate() {
            self.rms_power[channel] = self.rms_coefficient * self.rms_power[channel]
                + (1.0 - self.rms_coefficient) * sample * sample;
            self.peak[channel] = sample.abs().max(self.peak[channel] * self.peak_coefficient);
        }
    }
    pub fn rms(&self) -> [f32; 2] {
        self.rms_power.map(f32::sqrt)
    }
    pub fn peaks(&self) -> [f32; 2] {
        self.peak
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization_hits_minus_one_preserves_ratios_and_handles_silence() {
        for peak in [0.01, 0.5, 1.0, 2.0] {
            let n = Normalization::from_peak(peak).unwrap();
            assert!((amplitude_dbfs(peak * n.gain).unwrap() + 1.0).abs() < 1e-5);
            assert!(((peak * 0.25 * n.gain) / (peak * n.gain) - 0.25).abs() < 1e-6);
        }
        assert_eq!(Normalization::from_peak(0.0).unwrap().gain, 1.0);
        assert_eq!(amplitude_dbfs(0.0), None);
        assert!(Normalization::from_peak(f32::NAN).is_err());
        assert!(Normalization::from_peak(-1.0).is_err());
    }
    #[test]
    fn full_scale_sine_rms_uses_unit_amplitude_reference() {
        let rate = 48000;
        let mut meter = StereoMeter::new(rate);
        for i in 0..rate * 3 {
            let sample = (std::f32::consts::TAU * 1000.0 * i as f32 / rate as f32).sin();
            meter.push([sample, 0.0]);
        }
        assert!((amplitude_dbfs(meter.rms()[0]).unwrap() + 3.0103).abs() < 0.02);
        assert_eq!(amplitude_dbfs(meter.rms()[1]), None);
    }

    #[test]
    fn stereo_rms_and_peak_have_independent_channels_and_decay() {
        let rate = 48000;
        let mut meter = StereoMeter::new(rate);
        for _ in 0..rate * 3 {
            meter.push([0.5, 0.25]);
        }
        assert!((meter.rms()[0] - 0.5).abs() < 0.001);
        assert!((meter.rms()[1] - 0.25).abs() < 0.001);
        assert_eq!(meter.peaks(), [0.5, 0.25]);
        for _ in 0..rate {
            meter.push([0.0; 2]);
        }
        assert!(meter.rms()[0] < 0.1);
        assert!((amplitude_dbfs(meter.peaks()[0]).unwrap() - (-6.0206 - 20.0)).abs() < 0.05);
    }
}
