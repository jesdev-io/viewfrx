//! Tunable analysis/DSP policy. Mathematical identities and channel indices are
//! intentionally kept at their call sites; user-facing defaults and limits live here.
pub const NORMALIZE_TARGET_DBFS: f32 = -1.0;
pub const RMS_TIME_CONSTANT_SECONDS: f32 = 0.300;
pub const PEAK_RELEASE_DB_PER_SECOND: f32 = 20.0;
pub const DEFAULT_WAVEFORM_BINS: usize = 4096;
pub const MIN_WAVEFORM_BINS: usize = 2;
pub const MAX_WAVEFORM_BINS: usize = 65_536;
pub const ANALYSIS_CANCEL_INTERVAL: u64 = 4096;
pub const WAVEFORM_CACHE_VERSION: u32 = 1;
pub const WAVEFORM_CACHE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_WAVEFORM_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_SEEK_SECONDS: f64 = 1e9;

#[cfg(feature = "spectrogram")]
pub mod spectral {
    pub const DEFAULT_FFT_SIZE: usize = 2048;
    pub const FFT_SIZES: &[usize] = &[512, 1024, 2048, 4096, 8192];
    pub const HOP_DIVISOR: usize = 4;
    pub const MAX_COLUMNS: usize = 512;
    pub const MAX_FREQUENCY_BINS: usize = 256;
    pub const MAX_CHANNELS: usize = 64;
    pub const FLOOR_DBFS: f32 = -160.0;
    pub const CACHE_VERSION: u32 = 2;
    pub const CACHE_BYTES: u64 = 64 * 1024 * 1024;
    pub const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
    const _: () = assert!(MAX_COLUMNS.is_power_of_two());
    const _: () = assert!(HOP_DIVISOR >= 2);
    const _: () = assert!(MAX_FREQUENCY_BINS > 0);
}
