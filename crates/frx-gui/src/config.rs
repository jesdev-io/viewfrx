//! Desktop defaults, limits and geometry. Keep policy values here rather than
//! burying tunable numbers in event handling or painting code.
pub mod runtime {
    pub const MAX_FILES: usize = 100_000;
    pub const COMMAND_QUEUE: usize = 32;
    pub const SCAN_QUEUE: usize = 8;
    pub const SCAN_BATCH: usize = 128;
    pub const ANALYSIS_QUEUE: usize = 4;
    pub const POLL_MS: u64 = 10;
    pub const REPAINT_MS: u64 = 33;
    pub const BACKPRESSURE_MS: u64 = 2;
    pub const SEEK_THROTTLE_MS: u64 = 80;
    pub const KEY_SEEK_SECONDS: f64 = 5.0;
    pub const SMOKE_MIN_SECONDS: f64 = 1.0;
    pub const SMOKE_MAX_SECONDS: f64 = 120.0;
    pub const CAPTURE_TIMEOUT_SECONDS: f64 = 10.0;
    pub const LOG_CAPACITY: usize = 2_000;
}
pub mod controls {
    pub const GAIN_MIN_DB: f64 = -60.0;
    pub const GAIN_MAX_DB: f64 = 12.0;
    pub const GAIN_DEFAULT_DB: f64 = 0.0;
    pub const RATE_DEFAULT: f64 = 1.0;
    pub const AUTOPLAY_DEFAULT: bool = true;
    pub const RECURSIVE_DEFAULT: bool = true;
    pub const NORMALIZE_DEFAULT: bool = false;
    pub const HIGHPASS_DEFAULT: bool = false;
    pub const DEBUG_CONSOLE_DEFAULT: bool = false;
    #[cfg(feature = "spectrogram")]
    pub const SPECTRUM_DEFAULT: bool = true;
    /// Candidate dB grid steps (ascending); the smallest one that still fits
    /// the current panel height is used, so the grid adapts to resizing.
    #[cfg(feature = "spectrogram")]
    pub const SPECTRUM_DB_STEPS: &[f32] = &[3.0, 6.0, 10.0, 12.0, 15.0, 20.0];
    /// Minimum vertical pixels between dB grid lines.
    #[cfg(feature = "spectrogram")]
    pub const SPECTRUM_MIN_GRID_PX: f32 = 22.0;
    /// Lowest displayed frequency of the log-axis spectrum grid.
    #[cfg(feature = "spectrogram")]
    pub const SPECTRUM_MIN_HZ: f32 = frx_core::spectrum::MIN_GRID_HZ;
    /// Minimum zoom window length in seconds.
    pub const ZOOM_MIN_SECONDS: f64 = 0.01;
    /// Zoom step factor for the +/- buttons.
    pub const ZOOM_FACTOR: f64 = 2.0;
    /// Wheel zoom is delta-proportional and view-relative: one notch (~40 pt)
    /// scales the current view by 1 + ZOOM_WHEEL_STEP_PER_NOTCH.
    pub const ZOOM_WHEEL_NOTCH_POINTS: f32 = 40.0;
    pub const ZOOM_WHEEL_STEP_PER_NOTCH: f64 = 0.25;
    pub const ZOOM_WHEEL_MIN_FACTOR: f64 = 0.5;
    pub const ZOOM_WHEEL_MAX_FACTOR: f64 = 2.0;
    /// Live spectrum re-analysis interval while playing, in seconds.
    #[cfg(feature = "spectrogram")]
    pub const SPECTRUM_UPDATE_SECONDS: f64 = 0.1;
    pub const SCROLL_CONTEXT_ROWS: usize = 3;
    pub const SLIDER_DECIMALS: usize = 2;
    pub const DB_FLOOR: f32 = -60.0;
    pub const METER_WARN_DB: f32 = -12.0;
    pub const METER_HOT_DB: f32 = -3.0;
    pub const METER_TICKS: &[f32] = &[0.0, -6.0, -12.0, -18.0, -24.0, -36.0, -48.0, -60.0];
    #[cfg(feature = "spectrogram")]
    pub const SPECTRAL_FLOOR_DEFAULT: f64 = -100.0;
    #[cfg(feature = "spectrogram")]
    pub const SPECTRAL_FLOOR_MIN: f64 = -140.0;
    #[cfg(feature = "spectrogram")]
    pub const SPECTRAL_FLOOR_MAX: f64 = -40.0;
    #[cfg(feature = "spectrogram")]
    pub const SPECTRAL_CEILING_DB: f32 = 0.0;
}
pub mod layout {
    pub const WINDOW_SIZE: [f32; 2] = [1280.0, 800.0];
    pub const WINDOW_MIN: [f32; 2] = [900.0, 560.0];
    pub const HEADER_PADDING: i8 = 14;
    pub const HEADER_LOGO: f32 = 36.0;
    pub const BRAND_FONT: f32 = 25.0;
    pub const BRAND_SUBFONT: f32 = 12.0;
    pub const MICRO_FONT: f32 = 10.0;
    pub const SMALL_FONT: f32 = 11.0;
    pub const BODY_FONT: f32 = 14.0;
    pub const EMPTY_LOGO: f32 = 96.0;
    pub const EMPTY_TOP_FRACTION: f32 = 0.25;
    pub const BROWSER_WIDTH: f32 = 280.0;
    pub const BROWSER_MIN: f32 = 200.0;
    pub const BROWSER_MAX: f32 = 500.0;
    pub const SETTINGS_WIDTH: f32 = 240.0;
    pub const SETTINGS_MIN: f32 = 210.0;
    pub const SETTINGS_MAX: f32 = 320.0;
    pub const ROW_HEIGHT: f32 = 30.0;
    pub const CENTRAL_PADDING: i8 = 22;
    pub const CARD_PADDING: i8 = 10;
    pub const CARD_RADIUS: u8 = 5;
    pub const PLOT_RADIUS: u8 = 6;
    pub const SPACE_SMALL: f32 = 8.0;
    pub const SPACE_MEDIUM: f32 = 12.0;
    pub const SPACE_LARGE: f32 = 16.0;
    pub const SPACE_SECTION: f32 = 20.0;
    pub const EMPTY_GAP: f32 = 24.0;
    pub const COMBO_INSET: f32 = 10.0;
    pub const TAGS_MAX_HEIGHT: f32 = 100.0;
    pub const TRANSPORT_HEIGHT: f32 = 135.0;
    pub const WAVEFORM_CHROME_HEIGHT: f32 = 24.0;
    #[cfg(feature = "spectrogram")]
    pub const SPECTRAL_CHROME_HEIGHT: f32 = 28.0;
    pub const SETTINGS_ITEM_GAP: f32 = 6.0;
    pub const PLOT_MIN_HEIGHT: f32 = 100.0;
    pub const PLOT_MAX_HEIGHT: f32 = 380.0;
    #[cfg(feature = "spectrogram")]
    pub const SPECTRAL_HEIGHT_FRACTION: f32 = 0.5;
    pub const PLAY_SIZE: [f32; 2] = [110.0, 38.0];
    pub const GRID_X_DIVISIONS: usize = 10;
    pub const GRID_Y_DIVISIONS: usize = 4;
    pub const WAVE_SCALE: f32 = 0.43;
    pub const THIN_STROKE: f32 = 1.0;
    pub const PLAYHEAD_STROKE: f32 = 2.0;
    pub const METER_HEIGHT: f32 = 170.0;
    pub const METER_TOP: f32 = 18.0;
    pub const METER_BOTTOM: f32 = 32.0;
    pub const METER_LEFT: f32 = 12.0;
    pub const METER_BAR_WIDTH: f32 = 28.0;
    pub const METER_BAR_GAP: f32 = 18.0;
    pub const METER_AXIS_GAP: f32 = 12.0;
    pub const VALUE_MIN_WIDTH: f32 = 62.0;
    pub const ITEM_SPACING: [f32; 2] = [10.0, 8.0];
    pub const BUTTON_PADDING: [f32; 2] = [12.0, 7.0];
    pub const DEBUG_WINDOW_SIZE: [f32; 2] = [560.0, 260.0];
    #[cfg(feature = "spectrogram")]
    pub const SPECTRUM_HEIGHT: f32 = 130.0;
    /// Annotation column height when no spectrum column is shown.
    pub const ANNOTATION_ROW_DEFAULT: f32 = 64.0;
    /// Annotation column width as a fraction of the bottom area; the vertical
    /// split bar between the annotation and spectrum columns redistributes it.
    #[cfg(feature = "spectrogram")]
    pub const ANNOTATION_FRACTION_DEFAULT: f32 = 0.4;
    #[cfg(feature = "spectrogram")]
    pub const ANNOTATION_FRACTION_MIN: f32 = 0.25;
    #[cfg(feature = "spectrogram")]
    pub const ANNOTATION_FRACTION_MAX: f32 = 0.75;
    #[cfg(feature = "spectrogram")]
    pub const SPECTRUM_MIN_WIDTH: f32 = 260.0;
    /// Vertical split bar thickness in pixels.
    #[cfg(feature = "spectrogram")]
    pub const WIDTH_HANDLE: f32 = 8.0;
    /// Draggable height handle strip thickness in pixels.
    pub const HEIGHT_HANDLE: f32 = 8.0;
    pub const PLOT_HEIGHT_MIN: f32 = 60.0;
}
