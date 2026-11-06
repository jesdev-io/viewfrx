//! CLI defaults and polling/input bounds.
pub const WAVEFORM_CACHE: &str = ":temporary:";
#[cfg(feature = "spectrogram")]
pub const SPECTROGRAM_CACHE: &str = WAVEFORM_CACHE;
pub const STATUS_POLL_MS: u64 = 10;
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const BENCHMARK_SAMPLE_RATE: u32 = 48000;
