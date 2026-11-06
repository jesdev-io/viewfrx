//! CLI defaults and polling/input bounds.
pub const WAVEFORM_CACHE: &str = ".frxview/waveforms";
#[cfg(feature = "spectrogram")]
pub const SPECTROGRAM_CACHE: &str = ".frxview/waveforms/spectrograms";
pub const STATUS_POLL_MS: u64 = 10;
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const BENCHMARK_SAMPLE_RATE: u32 = 48000;
