use crate::{Frame, Shared, config::PREFERRED_DEVICE_BUFFER};
use anyhow::{Result, bail};
use cpal::{FromSample, SizedSample, traits::DeviceTrait};
use frx_core::levels::StereoMeter;
use std::sync::{Arc, atomic::Ordering};

pub(crate) fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    consumer: rtrb::Consumer<Frame>,
    shared: Arc<Shared>,
) -> Result<cpal::Stream> {
    match config.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(device, config, consumer, shared),
        cpal::SampleFormat::F64 => build::<f64>(device, config, consumer, shared),
        cpal::SampleFormat::I8 => build::<i8>(device, config, consumer, shared),
        cpal::SampleFormat::I16 => build::<i16>(device, config, consumer, shared),
        cpal::SampleFormat::I32 => build::<i32>(device, config, consumer, shared),
        cpal::SampleFormat::I64 => build::<i64>(device, config, consumer, shared),
        cpal::SampleFormat::U8 => build::<u8>(device, config, consumer, shared),
        cpal::SampleFormat::U16 => build::<u16>(device, config, consumer, shared),
        cpal::SampleFormat::U32 => build::<u32>(device, config, consumer, shared),
        cpal::SampleFormat::U64 => build::<u64>(device, config, consumer, shared),
        other => bail!("unsupported device sample format: {other:?}"),
    }
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    mut consumer: rtrb::Consumer<Frame>,
    shared: Arc<Shared>,
) -> Result<cpal::Stream> {
    let channels = config.channels() as usize;
    let error_shared = shared.clone();
    let mut meter = StereoMeter::new(config.sample_rate().0);
    let mut stream_config = config.config();
    stream_config.buffer_size = preferred_buffer_size(config.buffer_size());
    Ok(device.build_output_stream(
        &stream_config,
        move |data: &mut [T], _| {
            fill_metered(data, channels, &mut consumer, &shared, &mut meter);
        },
        move |_| {
            error_shared.device_error.store(true, Ordering::Release);
        },
        None,
    )?)
}

fn preferred_buffer_size(supported: &cpal::SupportedBufferSize) -> cpal::BufferSize {
    match *supported {
        cpal::SupportedBufferSize::Range { min, max } => {
            cpal::BufferSize::Fixed(PREFERRED_DEVICE_BUFFER.clamp(min, max))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    }
}

/// Callback kernel: bounded ring consumption, arithmetic, and atomics only.
fn fill_metered<T: SizedSample + FromSample<f32>>(
    data: &mut [T],
    channels: usize,
    consumer: &mut rtrb::Consumer<Frame>,
    shared: &Shared,
    meter: &mut StereoMeter,
) {
    let generation = shared.generation.load(Ordering::Acquire);
    // At most the fixed ring capacity can be stale. Also drain while paused or
    // stopped so a seek cannot leave the producer blocked behind old audio.
    for _ in 0..crate::RING_FRAMES {
        if consumer
            .peek()
            .is_ok_and(|frame| frame.generation != generation)
        {
            let _ = consumer.pop();
        } else {
            break;
        }
    }
    let playing = shared.active.load(Ordering::Acquire) && !shared.paused.load(Ordering::Acquire);
    let gain = f32::from_bits(shared.gain.load(Ordering::Relaxed));
    let normalize = f32::from_bits(shared.normalization_gain.load(Ordering::Relaxed));
    let mut peak = 0f32;
    let mut shortage = false;
    let mut clipped = 0;
    for output in data.chunks_mut(channels) {
        let frame = if playing {
            consumer.pop().ok().filter(|f| f.generation == generation)
        } else {
            None
        };
        let samples = if let Some(frame) = frame {
            shared
                .position
                .store(frame.position.to_bits(), Ordering::Release);
            shared
                .rendered_generation
                .store(frame.generation, Ordering::Release);
            frame.samples
        } else {
            if playing && !shared.done.load(Ordering::Acquire) {
                shortage = true;
            }
            [0.0; 2]
        };
        let mut measured = [0.0; 2];
        for (channel, sample) in output.iter_mut().enumerate() {
            let value = if channels == 1 {
                (samples[0] + samples[1]) * 0.5
            } else {
                samples.get(channel).copied().unwrap_or(0.0)
            } * normalize
                * gain;
            peak = peak.max(value.abs());
            if value.abs() > 1.0 {
                clipped += 1;
            }
            let actual = value.clamp(-1.0, 1.0);
            *sample = T::from_sample(actual);
            if channel < measured.len() {
                measured[channel] = actual;
            }
        }
        if channels == 1 {
            measured[1] = measured[0];
        }
        meter.push(measured);
    }
    for i in 0..2 {
        shared.rms[i].store(meter.rms()[i].to_bits(), Ordering::Relaxed);
        shared.channel_peak[i].store(meter.peaks()[i].to_bits(), Ordering::Relaxed);
    }
    shared.peak.store(peak.to_bits(), Ordering::Relaxed);
    if shortage {
        shared.underruns.fetch_add(1, Ordering::Relaxed);
    }
    shared.clipped.fetch_add(clipped, Ordering::Relaxed);
    if playing && consumer.is_empty() && shared.done.load(Ordering::Acquire) {
        shared.ended.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEST_RATE: u32 = 48000;
    fn fill<T: SizedSample + FromSample<f32>>(
        data: &mut [T],
        channels: usize,
        consumer: &mut rtrb::Consumer<Frame>,
        shared: &Shared,
    ) {
        fill_metered(
            data,
            channels,
            consumer,
            shared,
            &mut StereoMeter::new(TEST_RATE),
        );
    }

    #[test]
    fn normalization_is_post_buffer_and_stereo_meter_reads_actual_output() {
        let shared = Shared::default();
        shared.active.store(true, Ordering::Release);
        crate::set_normalization(&shared, Some(0.5)).unwrap();
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(2);
        producer
            .push(Frame {
                samples: [0.5, 0.25],
                position: 0.0,
                generation: 0,
            })
            .unwrap();
        let mut output = [0f32; 2];
        fill(&mut output, 2, &mut consumer, &shared);
        let status = crate::status(&shared);
        assert!((status.peak_dbfs[0].unwrap() + 1.0).abs() < 1e-5);
        assert!((status.peak_dbfs[1].unwrap() + 7.0206).abs() < 1e-4);
        assert!((output[0] / output[1] - 2.0).abs() < 1e-5);
    }
    #[test]
    fn buffer_request_is_small_but_respects_device_limits() {
        use cpal::{BufferSize, SupportedBufferSize};
        assert!(matches!(
            preferred_buffer_size(&SupportedBufferSize::Range { min: 32, max: 8192 }),
            BufferSize::Fixed(256)
        ));
        assert!(matches!(
            preferred_buffer_size(&SupportedBufferSize::Range {
                min: 512,
                max: 8192
            }),
            BufferSize::Fixed(512)
        ));
        assert!(matches!(
            preferred_buffer_size(&SupportedBufferSize::Range { min: 32, max: 128 }),
            BufferSize::Fixed(128)
        ));
        assert!(matches!(
            preferred_buffer_size(&SupportedBufferSize::Unknown),
            BufferSize::Default
        ));
    }
    #[test]
    fn stale_frames_silence_pause_gain_and_clipping() {
        let shared = Shared::default();
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(8);
        shared.generation.store(1, Ordering::Release);
        shared.active.store(true, Ordering::Release);
        producer
            .push(Frame {
                samples: [1.0, 1.0],
                position: 99.0,
                generation: 0,
            })
            .unwrap();
        producer
            .push(Frame {
                samples: [0.75, -0.75],
                position: 0.1,
                generation: 1,
            })
            .unwrap();
        shared.gain.store(2f32.to_bits(), Ordering::Relaxed);
        let mut data = [0f32; 2];
        shared.paused.store(true, Ordering::Release);
        fill(&mut data, 2, &mut consumer, &shared);
        assert_eq!(data, [0.0; 2]);
        assert_eq!(consumer.slots(), 1);
        shared.paused.store(false, Ordering::Release);
        fill(&mut data, 2, &mut consumer, &shared);
        assert_eq!(data, [1.0, -1.0]);
        assert_eq!(shared.clipped.load(Ordering::Relaxed), 2);
        assert_eq!(f64::from_bits(shared.position.load(Ordering::Relaxed)), 0.1);
        fill(&mut data, 2, &mut consumer, &shared);
        assert_eq!(data, [0.0; 2]);
        assert_eq!(shared.underruns.load(Ordering::Relaxed), 1);
        shared.done.store(true, Ordering::Release);
        fill(&mut data, 2, &mut consumer, &shared);
        assert!(shared.ended.load(Ordering::Relaxed));
        assert_eq!(shared.underruns.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn unsigned_silence_is_midpoint_and_extra_channels_are_silent() {
        let shared = Shared::default();
        let (_, mut consumer) = rtrb::RingBuffer::new(2);
        let mut unsigned = [0u16; 4];
        fill(&mut unsigned, 2, &mut consumer, &shared);
        assert_eq!(unsigned, [32768; 4]);
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(2);
        shared.active.store(true, Ordering::Release);
        producer
            .push(Frame {
                samples: [0.2, 0.4],
                position: 0.0,
                generation: 0,
            })
            .unwrap();
        let mut data = [0f32; 4];
        fill(&mut data, 4, &mut consumer, &shared);
        assert_eq!(data, [0.2, 0.4, 0.0, 0.0]);
    }
}
