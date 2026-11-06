//! Deterministic preview renderer, also used by the realtime producer.
use crate::config::{
    DC_BLOCK_CUTOFF_HZ, FADE_SECONDS, HIGHPASS_CUTOFF_HZ, MAX_MANUAL_GAIN, MAX_RATE, MIN_RATE,
};
use anyhow::{Result, bail};
use frx_core::decode::AudioReader;
use std::path::Path;

/// First-order highpass y[n] = a*(y[n-1] + x[n] - x[n-1]); a = exp(-2*pi*fc/fs).
/// At 5 Hz this is a DC blocker; at 20 Hz it removes infra-sound.
#[derive(Clone, Copy)]
struct OnePole {
    a: f32,
    y: f32,
    x_prev: f32,
}
impl OnePole {
    fn new(cutoff_hz: f32, sample_rate: u32) -> Self {
        let a = (-std::f32::consts::TAU * cutoff_hz / sample_rate.max(1) as f32).exp();
        Self {
            a,
            y: 0.0,
            x_prev: 0.0,
        }
    }
    fn reset(&mut self) {
        self.y = 0.0;
        self.x_prev = 0.0;
    }
    fn push(&mut self, x: f32) -> f32 {
        self.y = self.a * (self.y + x - self.x_prev);
        self.x_prev = x;
        self.y
    }
}

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
    /// Always-on DC blocker (per output channel).
    dc_block: [OnePole; 2],
    /// Optional fixed highpass against infra-sound.
    highpass: [OnePole; 2],
    highpass_enabled: bool,
    /// Anti-pop onset/offset fade: output frames since load/seek, total source
    /// frames (when the container reports them) and the fade length in frames.
    since_start: u64,
    total_frames: Option<u64>,
    fade_frames: f64,
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
            dc_block: [
                OnePole::new(DC_BLOCK_CUTOFF_HZ, output_rate),
                OnePole::new(DC_BLOCK_CUTOFF_HZ, output_rate),
            ],
            highpass: [
                OnePole::new(HIGHPASS_CUTOFF_HZ, output_rate),
                OnePole::new(HIGHPASS_CUTOFF_HZ, output_rate),
            ],
            highpass_enabled: false,
            since_start: 0,
            total_frames: reader.info().frames,
            fade_frames: FADE_SECONDS * reader.info().sample_rate as f64,
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
        self.since_start = 0;
        // A seek must not carry filter/fade transients across the jump.
        for f in self.dc_block.iter_mut().chain(self.highpass.iter_mut()) {
            f.reset();
        }
        Ok(())
    }

    /// Short onset/offset gain fade in the source domain: ramps up after
    /// load/seek and down over the last fade_frames of the recording.
    fn envelope(&self) -> f32 {
        let fade_in = if self.fade_frames > 0.0 {
            (self.since_start as f64 / self.fade_frames).min(1.0)
        } else {
            1.0
        };
        let fade_out = self.total_frames.map_or(1.0, |total| {
            let remaining = total as f64 - self.position * self.reader.info().sample_rate as f64;
            if self.fade_frames > 0.0 {
                (remaining / self.fade_frames).clamp(0.0, 1.0)
            } else {
                1.0
            }
        });
        (fade_in * fade_out) as f32
    }

    /// Toggle the fixed infra-sound highpass (the DC blocker is always on).
    pub fn set_highpass(&mut self, enabled: bool) {
        self.highpass_enabled = enabled;
    }
    pub fn highpass_enabled(&self) -> bool {
        self.highpass_enabled
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
        let sample: [f32; 2] =
            std::array::from_fn(|i| self.a[i] + (b[i] - self.a[i]) * self.phase as f32);
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
        // DSP chain on the output frame: remove DC offset always, infra-sound
        // highpass only when toggled, plus the anti-pop onset/offset fade.
        // All sections run at output rate; fades are computed in source time.
        let envelope = self.envelope();
        self.since_start += 1;
        let mut out = [0.0f32; 2];
        for i in 0..2 {
            let mut v = self.dc_block[i].push(sample[i]);
            if self.highpass_enabled {
                v = self.highpass[i].push(v);
            }
            out[i] = v * envelope;
        }
        Ok(Some(out))
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
    fn seek_resumes_at_the_right_position_with_clean_filter_state() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tone.wav");
        fixture(&path)?;
        let mut renderer = Renderer::open(&path, 8000, 1.0)?;
        let mut reference = Vec::new();
        while let Some(frame) = renderer.next_frame()? {
            reference.push(frame);
        }
        assert_eq!(reference.len(), 8000);
        // A seek restarts the DSP filters, so only the file start (zero filter
        // state) is bit-identical to a fresh render; mid-stream output differs
        // by the filter memory, which the anti-pop envelope covers in the engine.
        renderer.seek(0.0)?;
        assert_eq!(renderer.next_frame()?.unwrap(), reference[0]);
        renderer.seek(0.234)?;
        assert_eq!(renderer.position(), 0.234);
        let mut count = 0;
        while renderer.next_frame()?.is_some() {
            count += 1;
        }
        assert_eq!(count, 8000 - 1872);
        assert!(validate_gain(f32::NAN).is_err());
        assert!(validate_rate(0.0).is_err());
        assert!(validate_rate(f64::INFINITY).is_err());
        Ok(())
    }
    fn constant(path: &Path, value: i16, seconds: u32) -> Result<()> {
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for _ in 0..8000 * seconds {
            writer.write_sample(value)?;
        }
        writer.finalize()?;
        Ok(())
    }
    fn sine(path: &Path, hz: f32, seconds: u32) -> Result<()> {
        let rate = 8000;
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        for i in 0..rate * seconds {
            writer.write_sample(0.5 * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin())?;
        }
        writer.finalize()?;
        Ok(())
    }
    #[test]
    fn onset_and_offset_are_faded_to_avoid_dirac_pops() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("square.wav");
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for i in 0..8000 {
            writer.write_sample(if i % 2 == 0 { 16384i16 } else { -16384 })?;
        }
        writer.finalize()?;
        let mut renderer = Renderer::open(&path, 8000, 1.0)?;
        let first = renderer.next_frame()?.unwrap();
        assert!(first[0].abs() < 0.01, "onset must fade in: {first:?}");
        let mut mid = [0.0f32; 2];
        for _ in 0..4000 {
            mid = renderer.next_frame()?.unwrap();
        }
        assert!((mid[0].abs() - 0.5).abs() < 0.05, "steady state: {mid:?}");
        let mut last = mid;
        while let Some(frame) = renderer.next_frame()? {
            last = frame;
        }
        assert!(last[0].abs() < 0.01, "offset must fade out: {last:?}");
        // A seek restarts the onset fade, de-clicking the jump.
        renderer.seek(0.5)?;
        let at_seek = renderer.next_frame()?.unwrap();
        assert!(at_seek[0].abs() < 0.01, "seek onset fade: {at_seek:?}");
        Ok(())
    }
    #[test]
    fn dc_offset_is_removed_and_highpass_toggle_blocks_infra_sound() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let dc = dir.path().join("dc.wav");
        constant(&dc, 16384, 2)?; // constant +0.5: pure DC
        let mut renderer = Renderer::open(&dc, 8000, 1.0)?;
        let mut last = [1.0f32; 2];
        let mut sum = 0.0f64;
        let mut n = 0;
        while let Some(frame) = renderer.next_frame()? {
            last = frame;
            sum += frame[0] as f64;
            n += 1;
        }
        assert_eq!(n, 16000);
        assert!(last[0].abs() < 0.01, "DC must decay to silence: {last:?}");
        let mean = sum / n as f64;
        assert!(mean < 0.05, "mean must collapse: {mean}");
        // Toggle the fixed highpass: a 1 kHz tone passes, DC/low content is cut.
        let tone = dir.path().join("tone.wav");
        sine(&tone, 1000.0, 1)?;
        let mut renderer = Renderer::open(&tone, 8000, 1.0)?;
        renderer.set_highpass(true);
        assert!(renderer.highpass_enabled());
        let mut peak = 0.0f32;
        for _ in 0..4000 {
            if let Some(frame) = renderer.next_frame()? {
                peak = peak.max(frame[0].abs());
            }
        }
        assert!(
            (peak - 0.5).abs() < 0.03,
            "1 kHz tone must pass the 20 Hz highpass: {peak}"
        );
        let mut renderer = Renderer::open(&dc, 8000, 1.0)?;
        renderer.set_highpass(true);
        let mut tail_peak = 0.0f32;
        let mut count = 0;
        while let Some(frame) = renderer.next_frame()? {
            count += 1;
            if count > 14000 {
                // measure only the last 2000 frames: the startup transient is
                // expected and is covered by the anti-pop envelope in the engine
                tail_peak = tail_peak.max(frame[0].abs());
            }
        }
        assert_eq!(count, 16000);
        assert!(tail_peak < 0.01, "highpass must remove DC/lows: {tail_peak}");
        Ok(())
    }
}
