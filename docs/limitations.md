# Current limitations

[Documentation index](README.md) · [Roadmap](roadmap.md)

- Preferences are intentionally not persisted between sessions. The meter has a 300 ms exponential RMS envelope and decaying sample peaks; it is VU-style, **not an IEC-calibrated VU instrument**. Annotation metadata writing supports WAV and MP3; other formats report a clear error and are left untouched.
- Spectrograms are bounded, linearly spaced frequency overviews with maximum aggregation, not a zoomable analysis editor or power spectral density measurement.
- Playback supports mono/stereo; multichannel metadata/waveforms work, but auditioning >2 channels is rejected. Extra device channels are silent.
- WAV and MP3 are tested. Other Symphonia-enabled formats may work but are not yet qualified. A truncated stream ending at a valid packet boundary may look like normal EOF; this is not an audio-file integrity checker.
- Current-file read-ahead and a bounded overview cache exist. Next-file preloading and progressive multiresolution waveforms remain work.
- GUI discovery/analysis run on separate workers with cooperative cancellation and stale-result rejection. OS file reads, directory enumeration/sorting and device opening cannot be interrupted; a newer request may wait for one already in progress, but not on the UI thread. CLI analysis remains an explicit job.
- Cache identity uses path/size/mtime, not a full content hash. If files change while preserving both size and mtime, delete their cached analyses.
- Tagged releases provide portable archives plus Linux `.deb`/AppImage, Windows MSI, and macOS DMG packages. Windows and macOS packages are currently unsigned; Linux runtime graphics, desktop portal, and audio-driver libraries remain host-provided. Dependency/security audits and application licensing remain open work.
