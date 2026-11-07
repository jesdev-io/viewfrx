# Changelog

All notable changes to viewfrx. Versions follow SemVer; releases are tagged `v*` and published by CI with per-platform binaries (Linux/Windows/macOS) plus source.

## [0.1.3] - 2026-11-07

- Keep the hovered timestamp fixed at the same screen position during scroll-wheel zoom; cap zoom expansion at file edges. Button zoom behavior is unchanged.
- Replace the placeholder branding with the custom 15pt logo and gradient application/installer icon.
- Refresh README release details, add CI/release badges and AI policy link.

## [0.1.2] - 2026-11-07

- Add Linux `.deb`/AppImage, Windows MSI, and macOS DMG desktop packages to tagged releases (unsigned).
- Keep the existing cross-platform portable archives as a fallback.

## [0.1.1] - 2026-11-07

- Publish per-platform downloadable binary archives with SHA-256 checksums; include README and license.
- Document running release binaries without installing a Rust toolchain.

## [0.1.0] - first release

CLI-first audio browser (WAV/MP3) with a native egui desktop GUI, sharing one core/audio stack.

- Browse/import in place, filter, virtualized list; stateless sessions with temporary caches.
- Streaming playback with device selection, seek, pause/resume, gain (dB), pitch-changing rate, linked-channel −1 dBFS peak normalization on the DC-removed signal.
- DSP: DC offset removed during playback and reported in dBFS (`levels`), anti-pop onset/offset fades, optional fixed 20 Hz highpass (`--highpass` / session `highpass` / GUI toggle).
- Cached bounded waveform overviews (`waveform`, `--channel/--stereo`, `--from/--to` zoom windows) and optional `spectrogram` feature (`spectrogram`, `spectrum`, 1/3-octave log-frequency output).
- GUI: JESDEV theme, menu bar (File/Playback/View/Help→About), symbol transport buttons, vertical stereo dBFS meters, grouped L/R/Both waveform+spectrogram plots (waveforms on top), draggable plot heights, tandem time zoom (buttons/wheel), live log-frequency spectrum column with 1/3-octave grid + HPF curve, annotation field + right-click markers (`[hh:mm:ss:cc]`, tag symbol, hover preview), floating annotations saved via Ctrl+S dialog, debug console window, `--verbose` CLI logging.
- Timestamp annotations stored in file metadata (WAV LIST/INFO, MP3 ID3v2) — atomic, non-destructive, idempotent; CLI `annotate`/`annotations` (+ `--output`, `--clear`) and session ops.
- JSON-lines `session` protocol; `benchmark` for throughput; reproducible `scripts/check-performance.py` and `scripts/check-gui.py`.
- CI on GitHub Actions: fmt + clippy + tests (both feature configs) + release builds on Linux/Windows/macOS; `v*` tags publish releases with binaries + source.
