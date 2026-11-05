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
5. **Headless feature parity:** editable sidecar annotations, live meter ballistics, configurable FFT/spectrogram analysis, offline render/export for numerical/audio comparisons. No source metadata writes until format-specific safety and backup policy is agreed.
6. **Coverage:** more sample formats/rates, corrupt/truncated files, large metadata, empty sources, realistic BWF field recordings, high-channel-count audio, device loss and sustained load. Confirm all enabled codecs and selected crate features/licenses.

## Delivered: initial GUI

- Native egui/eframe desktop frontend; independently usable CLI is retained.
- JESDEV dark compact layout: browser left; metadata, waveform and transport center; device/settings/output peak right.
- Shared core/audio APIs, background device management, bounded scan/analysis workers and stale-response rejection.
- Virtualized/filterable alphabetical file lists, keyboard navigation, waveform click/drag seeking, file/folder choosers and drag-and-drop.
- Logo bundled with source provenance; redistribution rights still need confirmation before publishing.
- Hardware-independent model, worker, input-routing and rendering tests; muted native 10,000-candidate screenshot smoke verified on Linux.

## Next GUI work

- Spectrum/spectrogram, annotated metadata editing and calibrated VU ballistics, after adding their headless APIs.
- Waveform zoom/pan and progressive analysis, accessibility/screen-reader testing, persisted user preferences.
- Library-root removal/management; clearer operation-completion events and more stress testing during repeated selections.
- Native chooser and drag/drop end-to-end tests, sustained playback/UI load tests, HiDPI/multi-monitor coverage and Windows/macOS qualification.

## Distribution

- Native Windows/macOS build and runtime checks (not established by Linux tests).
- Remote/CI only after the owner supplies the intended hosting location.
- Dependency/security audit, application license, packaging/signing/update policy.
