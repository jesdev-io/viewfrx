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
4. **Library/analysis:** cancelable background scans and progressive multiresolution waveforms, cache concurrency/version migration tests, metadata caching and source-change detection. Deduplicate overlapping imported roots without unbounded result buffering.
5. **Headless feature parity:** editable sidecar annotations, live meter ballistics, configurable FFT/spectrogram analysis, offline render/export for numerical/audio comparisons. No source metadata writes until format-specific safety and backup policy is agreed.
6. **Coverage:** more sample formats/rates, corrupt/truncated files, large metadata, empty sources, realistic BWF field recordings, high-channel-count audio, device loss and sustained load. Confirm all enabled codecs and selected crate features/licenses.

## Then: GUI

- Evaluate egui versus alternatives with the engine already running independently.
- JESDEV dark compact layout: browser left; metadata, waveform and transport center; settings/meters right; optional spectrogram above timeline.
- All operations through shared APIs; no playback logic in widgets.
- Keyboard-first browsing, virtualized large lists, progressive analysis and visible pending/error states.
- Import branding assets with source/provenance and confirm redistribution rights.

## Distribution

- Native Windows/macOS build and runtime checks (not established by Linux tests).
- Remote/CI only after the owner supplies the intended hosting location.
- Dependency/security audit, application license, packaging/signing/update policy.
