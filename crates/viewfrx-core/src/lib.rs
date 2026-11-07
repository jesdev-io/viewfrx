//! Headless audio browsing primitives, shared by every frontend.
pub mod annotations;
mod cache;
pub mod catalog;
pub mod config;
pub mod decode;
pub mod levels;
#[cfg(feature = "spectrogram")]
pub mod spectrogram;
#[cfg(feature = "spectrogram")]
pub mod spectrum;
pub mod waveform;
