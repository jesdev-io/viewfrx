# Next milestones

## Delivered: first headless slice

- Local Git history, separate core/audio/CLI crates, locked dependencies.
- File/folder imports in place, streaming discovery, metadata probing.
- WAV/MP3 streaming audition with device selection, seek, pause/resume, gain, pitch-changing rate, stop, status and recoverable file errors.
- Bounded current-file read-ahead, generation-based stale audio rejection, small supported device buffers.
- Bounded waveform overview and persistent, budgeted, source-invalidated cache.
- Hardware-independent unit/CLI integration tests, reproducible benchmarks and muted Linux hardware smoke checks.
- Reference/branding inventory and honest performance limitations.

## Before calling the CLI engine complete

1. **Audio quality and transport:** bandlimited sample-rate/rate conversion; de-click transitions; a separately named pitch-preserving time-stretch mode with reference tests; explicit multichannel layout/downmix policy.
2. **Scheduling:** remove measured transition shortages through proper preparation/swap, cancellable/priority work, next-file preloading; stress-test rapid random seek/switch and shutdown on slow media. Keep memory/queue bounds explicit.
3. **Command completion:** operation IDs and completion events so clients can await their own request rather than infer completion from generations. Stable device identifiers, output rate/buffer configuration, persisted settings, device reconnection.
4. **Library/analysis:** progressive multiresolution waveforms, cache concurrency/version migration tests, metadata caching and source-change detection. GUI scan/analysis cancellation and bounded, deduplicated browsing are implemented; cancellation during blocked OS reads remains limited.
5. **Headless feature parity:** a separate magnitude-spectrum view, offline render/export for numerical/audio comparisons. Shared stereo RMS/peak ballistics, peak normalization and feature-gated configurable STFT analysis are implemented. No source metadata writes until format-specific safety and backup policy is agreed.
6. **Coverage:** more sample formats/rates, corrupt/truncated files, large metadata, empty sources, realistic BWF field recordings, high-channel-count audio, device loss and sustained load. Confirm all enabled codecs and selected crate features/licenses.

## Delivered: initial GUI

- Native egui/eframe desktop frontend; independently usable CLI is retained.
- JESDEV dark compact layout: browser left; metadata, waveform and transport center; device/settings/output peak right.
- Shared core/audio APIs, background device management, bounded scan/analysis workers and stale-response rejection.
- Virtualized/filterable alphabetical file lists, keyboard navigation, waveform click/drag seeking, file/folder choosers and drag-and-drop.
- Logo bundled with source provenance; redistribution rights still need confirmation before publishing.
- Hardware-independent model, worker, input-routing and rendering tests; muted native 10,000-candidate screenshot smoke verified on Linux.

## Delivered: level/normalization/spectrogram slice

- Double-click parameter reset, dB gain, vertical stereo dBFS RMS/peak meters.
- Original waveform plus subdued −1 dBFS peak-normalized overlay; matching linked-channel playback gain and cancellable analysis-dependent audition.
- Optional `spectrogram` feature across core/CLI/GUI: bounded cached streaming Hann STFT, configurable FFT size/display floor, calibrated source levels and background color conversion.
- Centralized production tuning/layout constants; source data is never modified.
- Debug/release tests with feature coverage, muted normalization/device checks and native spectral screenshots. See [analysis.md](analysis.md).

## Delivered: requirements round (DSP, channels, annotations, debug, menu)

- DC offset reported in dBFS as a file parameter and removed during playback (DC blocker); normalization uses the DC-removed peak.
- Short onset/offset anti-pop fades; optional fixed 20 Hz highpass toggle (CLI `play --highpass`, session `highpass`, GUI toggle).
- Left/Right/Both selection for waveform and spectrogram, grouped as wave L/R and spec L/R; CLI `waveform/spectrogram --channel/--stereo`.
- Timestamp annotations in the [dd:hh:mm:ss] format stored in WAV/MP3 file metadata (atomic, non-destructive, idempotent); CLI `annotate`/`annotations` plus session ops; GUI text field + right-click markers + plot lines.
- Symbol transport buttons, top-bar menu exporting main CLI actions, debug console window (GUI) and `--verbose` log lines (CLI).
- Stateless sessions: no catalog persistence, temporary default analysis caches.

## Delivered: spectrum, zoom/heights, annotation save workflow

- Live configurable magnitude spectrum (core `spectrum` analysis, CLI `spectrum`, GUI panel bottom-right of the center area with toggle and HPF indicator), updated at the playhead while playing.
- Waveform and spectrogram heights are resizable by dragging their bottom edges; one shared time-zoom window scales all waveforms and spectrograms in tandem (zoom buttons or scroll wheel over a plot; double-click resets/rewinds). CLI parity via `waveform/spectrogram --from/--to`.
- Annotations are floating while a file is viewed; Ctrl+S / File menu opens a standard save dialog (naming + overwrite warning) that persists them atomically; switching files with unsaved changes warns (Save / Dismiss / Cancel). CLI: `annotate` (in place or `--output` copy) and `annotations`.
- Resource metrics (binary size, peak RSS, latency) measured and documented; CPAL confirmed as the non-EOL backend choice.

## Next GUI work

- Separate magnitude spectrum, annotated metadata editing and an optional IEC-calibrated VU mode, after adding their headless APIs.
- Waveform zoom/pan and progressive analysis, accessibility/screen-reader testing, persisted user preferences.
- Library-root removal/management; clearer operation-completion events and more stress testing during repeated selections.
- Native chooser and drag/drop end-to-end tests, sustained playback/UI load tests, HiDPI/multi-monitor coverage and Windows/macOS qualification.

## Distribution

- Native Windows/macOS build and runtime checks (not established by Linux tests).
- Remote/CI only after the owner supplies the intended hosting location.
- Dependency/security audit, application license, packaging/signing/update policy.
