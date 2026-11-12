# Desktop guide

[Documentation index](README.md)

```sh
cargo run --release -p viewfrx-gui --features spectrogram -- /path/to/recordings
# Or build both binaries with spectral analysis enabled:
cargo build --release --workspace --features spectrogram --locked
./target/release/viewfrx-gui
```

- **Left:** virtualized, filterable file list, recursive browsing and optional audition-on-selection. Use the **File** menu (Import files/folder, Refresh), drag-and-drop, or startup paths to import. First selection on startup does not auto-play.
- **Center:** metadata cards (including the DC offset in dBFS), waveforms first (DC-removed, with a subdued −1 dBFS normalized overlay), then optional source spectrogram(s), click/drag seeking, right-click timestamp markers, symbol transport buttons (▶ play, ❚❚ pause, ■ stop, ⏮/⏭ browse), and a bottom area split into two columns — annotations on the left, the live spectrum on the right — with a vertical resize bar between them.
- **Stereo views:** waveform and spectrogram each get a **Left / Right / Both** selector and are grouped as WAVE L, WAVE R, SPEC L, SPEC R (stereo recordings only).
- **Resizable/zoomable plots:** drag a plot's bottom edge to resize it; zoom with the −/+ buttons or the scroll wheel over a plot. Scroll zoom keeps the timestamp under the pointer at the same horizontal position; all waveforms and spectrograms share the zoom window, and seeking/playhead/markers/time readout follow it. Double-click a plot rewinds and resets the zoom.
- **Live spectrum:** a toggleable magnitude-spectrum column at the bottom right of the center area (log-frequency 1/3-octave grid, height-adaptive dB grid, highpass response curve when enabled), recomputed at the playhead while playing; the vertical bar between it and the annotation column redistributes their widths.
- **Right:** output device, gain in dB, Normalize, pitch-changing rate, fixed highpass toggle, vertical stereo RMS/peak meters in dBFS, FFT size and spectral display floor, and a debug-console toggle.
- **Top bar:** File / Playback / View menus export the main CLI-equivalent actions.
- **Annotations:** type `[hh:mm:ss:cc] note` (cc = centiseconds) in the bottom field (Enter) or right-click a plot at a position; markers render as vertical lines and in the field and are idempotent. They stay **floating** until you save with **Ctrl+S** or **File → Save annotations**, which opens a standard save dialog (naming + overwrite warning) and writes them into the file's own metadata (WAV LIST/INFO, MP3 ID3v2) — the file stays a normal playable audio file. Switching files with unsaved annotations warns first (Save / Dismiss / Cancel).
- **Double-click to reset:** gain returns to 0 dB/unity and unmutes; rate to 1×; toggles, device and spectral settings to their defaults. Double-click a plot to rewind.
- **Keys:** Space play/pause; Up/Down browse visible results; Left/Right seek five seconds; Esc stop. Shortcuts do not intercept text entry.
- **JESDEV-inspired theme:** dark compact panels, amber waveform/actions, purple playhead, teal annotation markers; custom viewfrx logo and gradient app icon from the included Draw.io sources.

Sessions are stateless: opening files/folders replaces the current roots, Refresh rescans them, and nothing is restored on restart. Analysis caches are temporary by default. `--cache DIR` explicitly retains analysis artifacts for diagnostics; deprecated `--library` is ignored. Overlapping roots are deduplicated and displayed candidates are capped at 100,000, with an explicit warning. Device switches stop playback. Output failure does not prevent browsing or analysis. `--muted` starts at gain zero.

On Linux the GUI uses egui/eframe with OpenGL and X11/Wayland runtime libraries; file dialogs use the desktop portal (`xdg-desktop-portal` plus a desktop-specific backend). Drag-and-drop and startup paths remain alternatives if a desktop portal is unavailable. Windows/macOS builds run in CI, but their runtime audio behavior and native dialogs have not yet been qualified.

The `spectrogram` Cargo feature is optional; omit it for a waveform-only build. Feature builds can show the spectrogram (off by default; enable via the settings toggle or `--spectrogram`). See [GUI design, tests and limitations](gui.md) and [level/normalization/spectral conventions](analysis.md).

