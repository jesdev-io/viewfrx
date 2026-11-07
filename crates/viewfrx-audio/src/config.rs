//! Transport and realtime scheduling policy.
pub const RING_FRAMES: usize = 8192;
pub const COMMAND_CAPACITY: usize = 32;
pub const PREFERRED_DEVICE_BUFFER: u32 = 256;
pub const DECODE_BATCH_FRAMES: usize = 256;
pub const WORKER_POLL_MS: u64 = 1;
pub const MIN_RATE: f64 = 0.25;
pub const MAX_RATE: f64 = 4.0;
pub const MAX_MANUAL_GAIN: f32 = 4.0;
/// One-pole DC blocker, always on: DC offset is removed from playback (and
/// reported as a file parameter in dBFS by analysis) instead of steering gain.
pub const DC_BLOCK_CUTOFF_HZ: f32 = 5.0;
/// Optional fixed highpass cutoff for infra-sound content (toggle).
pub const HIGHPASS_CUTOFF_HZ: f32 = 20.0;
/// Short onset/offset gain fade applied to the first/last source samples of a
/// recording (and after every seek), so playback never starts or stops on a
/// full-scale step (dirac-pop). Applied in the renderer, not the audio callback.
pub const FADE_SECONDS: f64 = 0.010;
