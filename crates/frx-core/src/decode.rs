use std::{fs::File, path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{Decoder, DecoderOptions},
    errors::Error,
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::{Time, TimeBase},
};

#[derive(Debug, Clone, Serialize)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: usize,
    pub frames: Option<u64>,
    pub duration_seconds: Option<f64>,
    pub bits_per_sample: Option<u32>,
    pub tags: Vec<(String, String)>,
}

/// A packet-streaming decoder. Its allocation is bounded by codec packet size,
/// not recording duration. Do not call this from an audio callback.
pub struct AudioReader {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track: u32,
    time_base: TimeBase,
    info: AudioInfo,
    samples: Vec<f32>,
    cursor: usize,
    discard_before: Option<u64>,
}

impl AudioReader {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            hint.with_extension(ext);
        }
        let probed = symphonia::default::get_probe().format(
            &hint,
            MediaSourceStream::new(Box::new(file), Default::default()),
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )?;
        let mut format = probed.format;
        let track = format.default_track().context("no default audio track")?;
        let params = &track.codec_params;
        let rate = params.sample_rate.context("unknown sample rate")?;
        let channels = params.channels.context("unknown channel layout")?.count();
        if rate == 0 || channels == 0 {
            bail!("invalid audio stream configuration");
        }
        let time_base = params.time_base.unwrap_or(TimeBase::new(1, rate));
        let duration = params.n_frames.map(|n| {
            let time = time_base.calc_time(n);
            time.seconds as f64 + time.frac
        });
        let info = AudioInfo {
            sample_rate: rate,
            channels,
            frames: params.n_frames,
            duration_seconds: duration,
            bits_per_sample: params.bits_per_sample,
            tags: Vec::new(),
        };
        let id = track.id;
        let decoder = symphonia::default::get_codecs().make(params, &DecoderOptions::default())?;
        let mut reader = Self {
            format: {
                // Container tags are available without decoding the full recording.
                format.metadata().skip_to_latest();
                format
            },
            decoder,
            track: id,
            time_base,
            info,
            samples: Vec::new(),
            cursor: 0,
            discard_before: None,
        };
        if let Some(metadata) = reader.format.metadata().current() {
            reader.info.tags = metadata
                .tags()
                .iter()
                .map(|tag| {
                    (
                        tag.std_key
                            .map(|key| format!("{key:?}"))
                            .unwrap_or_else(|| tag.key.clone()),
                        tag.value.to_string(),
                    )
                })
                .collect();
        }
        if let Some(mut metadata) = probed.metadata.into_inner()
            && let Some(revision) = metadata.metadata().current()
        {
            reader.info.tags.extend(
                revision
                    .tags()
                    .iter()
                    .map(|tag| (tag.key.clone(), tag.value.to_string())),
            );
        }
        Ok(reader)
    }

    pub fn info(&self) -> &AudioInfo {
        &self.info
    }

    pub fn seek(&mut self, seconds: f64) -> Result<()> {
        if !seconds.is_finite() || !(0.0..=1e9).contains(&seconds) {
            bail!("seek must be finite and between 0 and 1e9 seconds");
        }
        let seconds = self
            .info
            .duration_seconds
            .map_or(seconds, |d| seconds.min(d));
        let result = self.format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: Time::from(Duration::from_secs_f64(seconds)),
                track_id: Some(self.track),
            },
        )?;
        self.decoder.reset();
        self.samples.clear();
        self.cursor = 0;
        self.discard_before = Some(result.required_ts);
        Ok(())
    }

    /// Decode one interleaved source frame into a caller-owned slice.
    pub fn next_frame(&mut self, frame: &mut [f32]) -> Result<bool> {
        if frame.len() != self.info.channels {
            bail!("frame channel count mismatch");
        }
        loop {
            if self.cursor + frame.len() <= self.samples.len() {
                frame.copy_from_slice(&self.samples[self.cursor..self.cursor + frame.len()]);
                self.cursor += frame.len();
                return Ok(true);
            }
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(error) => return Err(error.into()),
            };
            if packet.track_id() != self.track {
                continue;
            }
            let decoded = self.decoder.decode(&packet)?;
            if decoded.spec().rate != self.info.sample_rate
                || decoded.spec().channels.count() != self.info.channels
            {
                bail!("midstream format changes are not supported");
            }
            let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
            buffer.copy_interleaved_ref(decoded);
            self.samples.clear();
            self.samples.extend_from_slice(buffer.samples());
            // Reject non-finite PCM instead of propagating NaNs into output/meters.
            if self.samples.iter().any(|s| !s.is_finite()) {
                bail!("non-finite audio sample");
            }
            self.cursor = 0;
            if let Some(target) = self.discard_before {
                if packet.ts() < target {
                    let delta = self.time_base.calc_time(target - packet.ts());
                    let skip = ((delta.seconds as f64 + delta.frac) * self.info.sample_rate as f64)
                        .round() as usize;
                    self.cursor = skip
                        .saturating_mul(self.info.channels)
                        .min(self.samples.len());
                    if self.cursor == self.samples.len() {
                        continue;
                    }
                }
                self.discard_before = None;
            }
        }
    }
}

pub fn inspect(path: &Path) -> Result<AudioInfo> {
    Ok(AudioReader::open(path)?.info)
}
