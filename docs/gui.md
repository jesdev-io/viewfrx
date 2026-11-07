# Native desktop GUI

![Native viewfrx with a real synthetic chirp spectrogram and normalized waveform overlay](assets/analysis.png)

Actual Linux framebuffer capture from the muted `--demo` smoke test, not a mockup. Both plots are calculated from a generated stereo WAV; placeholder list entries are not decoded recordings. The [initial waveform-only screenshot](assets/gui.png) is retained for historical comparison.

## Choice and scope

The GUI uses **egui/eframe 0.33 with Glow/OpenGL**, not a browser shell. It fits the existing Rust architecture, supports custom waveform painting and virtualized lists, and does not require moving application logic into a JavaScript frontend. WGPU was not needed for a bounded waveform overview. `rfd` provides file/folder dialogs; Linux uses the desktop portal. This is a pragmatic selection, not a benchmark proving egui faster than every alternative.

References checked:
- https://docs.rs/eframe/0.33.3/eframe/
- https://docs.rs/rfd/0.15.4/rfd/
- JESDEV source CSS/logo inventory in [references.md](references.md).

The bundled logo is copied unchanged from the requested sibling repository. Its provenance is in `crates/viewfrx-gui/assets/README.md`. No new asset license is invented.

## Run

```sh
cargo run --release -p viewfrx-gui --features spectrogram -- /path/to/recordings
# Separate catalog, if desired:
./target/release/viewviewfrx-gui --library /tmp/viewfrx-library.json --cache /tmp/viewfrx-cache /path/to/audio
# Silent initial gain (can still be changed in the UI):
./target/release/viewviewfrx-gui --muted
```

Imports register source paths in place. Initial discovery selects the first file for metadata/waveform inspection without starting playback. Clicking a file or using Up/Down auditions it when the checkbox is enabled. Native chooser operations run off-thread; startup paths and drag/drop avoid reliance on a desktop portal.

GUI defaults to recursive traversal, alphabetical order within each directory and result deduplication; CLI defaults remain unchanged. `viewfrx list --sort --recursive` exposes the ordered traversal headlessly. Filtering searches filenames and full paths. Keyboard selection scrolls into view. The list displays at most 100,000 candidates; it reports the limit rather than silently claiming a complete listing.

Settings cover output selection/retry, gain −60 to +12 dB plus mute, linked-channel −1 dBFS peak normalization on the DC-removed signal, rate 0.25–4×, the fixed highpass toggle, vertical stereo RMS/peak dBFS meters, the live spectrum toggle, and a debug-console window toggle. Transport buttons use symbols (▶ play, ❚❚ pause, ■ stop, ⏮/⏭ browse); a File/Playback/View top-bar menu exports the main actions. Waveform and spectrogram each offer Left/Right/Both and are grouped (WAVE L/R, SPEC L/R). The bottom annotation field accepts `[hh:mm:ss:cc] note` (Enter) and right-clicking a plot adds a marker at that time; markers render as vertical lines and are stored in the file's metadata (see [analysis.md](analysis.md)). Metadata cards list the DC offset in dBFS. The `spectrogram` feature adds a source spectrogram with FFT-size and display-floor controls. Double-click sliders/value labels, toggles or selectors to reset; double-click either plot to rewind. Gain reset restores unity and unmutes. CLI linear gain still supports 0–4×.

The subdued waveform shows exactly the auto gain used by Normalize. Waveform and spectrogram heights are resizable by dragging the handle at each plot's bottom edge; a single zoom window (buttons or scroll wheel over any plot) stretches a time subsection across all plots, and seeking/playhead/annotation lines/time readout all follow it. The live magnitude spectrum sits at the bottom right of the center area (left of the options box), is resizable via its own bottom drag handle, shows a log-frequency grid with 1/3-octave center frequencies and a dB grid whose step adapts to the panel height, plots the fixed highpass response curve when enabled, and is recomputed at the playhead during playback. Annotations are floating until saved via Ctrl+S / File menu (standard save dialog with overwrite warning); switching files with unsaved annotations warns first. Uncached normalized audition waits for background peak analysis without freezing the UI; Cancel/Stop cancels that intent, and pending seeks are preserved. Device changes stop the current stream and cancel pending playback. Press Play after the replacement is ready. Buffer/rate configuration and stable persistent device IDs remain pending. The RMS/peak meter is VU-style, **not** an IEC-calibrated VU instrument. Rate changes pitch and uses preview interpolation. See [analysis conventions](analysis.md).

## Worker boundaries

- **Audio service:** owns CPAL/Engine on its own thread; typed commands in, latest status out. Device discovery/open/drop never runs in egui `update()`.
- **Library worker:** loads/saves the shared catalog, performs sorted traversal, deduplicates and emits bounded batches. New scans supersede old ones; queued imports remain additive.
- **Analysis worker:** probes metadata, generates/loads waveform, then optionally computes/loads STFT and converts it to a bounded color image. Every response carries a selection/settings generation. Cancelled work cannot replace newer UI state or persist a partial analysis.
- **Views:** model/filter/navigation, virtualized rows, bounded waveform painting, texture upload and transport-intent routing. No source decoding, FFT computation or filesystem traversal in widgets.

Per-directory sorting and OS reads may still take time before cancellation is noticed. The UI remains independent, but a new analysis can wait behind an in-flight read on the analysis worker. Query filtering is currently a synchronous in-memory pass over up to 100,000 cached path strings; further performance work should measure real long-path collections rather than extrapolate from the smoke fixture.

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features spectrogram --locked -- -D warnings
cargo test --workspace --features spectrogram --locked
cargo test --workspace --no-default-features --locked
cargo build --release --workspace --features spectrogram --locked
python3 scripts/check-gui.py --expect-spectrogram --demo --screenshot /tmp/viewfrx.png
python3 scripts/check-performance.py --audio --spectrogram
```

The feature-enabled suite has **43 tests**, with **35** in the waveform-only configuration. Coverage includes:
- Filtering across incoming batches and navigation through visible results.
- Input validation for timeline mapping.
- Actual egui pointer input on the waveform and Space-key routing to shared transport commands.
- Empty/populated views at 1280×800 and 900×560 with no audio device.
- Sorted/deduplicated background discovery, latest-selection metadata/waveform ordering and cancellation under result-queue backpressure.
- Mid-analysis cancellation without writing a partial cache entry.
- Double-click gain reset, shared normalized-start gain, waiting/seek routing, cancellation on Stop/selection changes, and immediate ordinary playback after disabling normalization.
- Stereo RMS/peak calibration/decay, actual normalized output, spectral calibration, anti-phase preservation, transient timing, bounds/cache validation and worker-side image delivery.

The spectrogram smoke also asserts that the feature is compiled and a texture is ready. [Feature-enabled run](spectrogram-gui-baseline.json): 10,000 candidates, both analyses complete, 193 updates, maximum measured update CPU ~0.86 ms (3.36 s total including the deliberate three-second hold). The generated chirp/burst screenshot is produced by the same script with `--demo`; its timing is recorded separately in [the demo run](analysis-demo-baseline.json). No audio playback is triggered by either visual smoke.

For comparison, the original native smoke launches an actual window, uses temporary sources/catalog/cache, renders 10,000 candidates and a real decoded MP3 waveform, captures a real framebuffer PNG and closes automatically. It starts muted and does not trigger playback. The screenshot was visually inspected. [Raw run](gui-baseline.json): all 10,000 candidates loaded, 143 UI updates, largest measured `update()` CPU duration ~0.98 ms. This excludes GPU rendering, OS scheduling, window creation and screenshot encoding; it is **not** a frame-latency guarantee. Total process duration was ~9.2 s including the deliberate three-second hold, native startup, capture and teardown. A repeat also passed (~0.75 ms max update CPU, ~8.9 s total).

The separate muted CPAL regression passed after these changes: WAV/MP3 playback, seeking, pause/resume, rate control, normalized start/live normalization, EOF and file-error recovery; no additional shortages during its one-second steady-playback observation. Offline callback tests verify nonzero channel levels without emitting sound. This is not sustained-load testing or subjective listening.

Linux startup emitted ALSA/JACK enumeration warnings for unavailable optional devices; the default output remained usable. Native chooser interaction, drag/drop interaction, accessibility, HiDPI behavior and Windows/macOS runtime behavior have **not** been end-to-end verified.

## Diagnostics

`--smoke-seconds N` (1–120) prints a JSON UI summary and closes automatically. With `--screenshot PATH.png`, it captures the rendered window before exiting; capture failure/timeout exits nonzero. These diagnostics exercise the normal application and do not substitute fabricated waveform data. `scripts/check-gui.py` requires a display; on a configured CI machine it can be wrapped with `xvfb-run`.

## Remaining work

A separate magnitude-spectrum panel, annotation editor, IEC-calibrated VU mode, waveform/spectral zoom/pan, pitch-preserving stretching and persisted playback settings remain work. No claim of glitch-free operation on network drives or every audio driver. The backend limitations in [roadmap.md](roadmap.md) still apply.
