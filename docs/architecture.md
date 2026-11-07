# Architecture

## Boundaries

- `viewfrx-core`: filesystem discovery, streaming decoding/probing, level/normalization conventions, timestamp annotations in file metadata, cached waveform and optional spectral analysis. No device, UI, or argument-parsing dependencies.
- `viewfrx-audio`: typed transport commands, streaming preview engine, CPAL device adapter. No CLI or UI dependencies.
- `viewfrx-cli`: Clap and JSON-lines adapters.
- `viewfrx-gui`: egui views and background adapters over the same Rust APIs, never subprocess playback.

Four focused crates, not a generic plugin/command framework.

## Audio contract

Disk access, seeking and decoding belong on a producer thread. CPAL consumes a bounded SPSC ring; the callback never takes a mutex, reads a file, decodes, logs, or allocates. Atomics hold transport flags and telemetry. Each buffered frame carries a generation: old audio is discarded after seek/switch/stop. Command submission is bounded and reports overload rather than blocking. Worker errors are visible in status. Shutdown joins the worker (local filesystem I/O can still delay shutdown).

The renderer always removes DC offset (one-pole DC blocker), applies a short onset/offset fade, and optionally a fixed infra-sound highpass (`Highpass` command; status reports it). Rate adjustment changes pitch and duration together. The first preview renderer uses linear interpolation, not mastering-quality antialiased resampling or pitch-preserving time stretching. These are explicit follow-up DSP tasks, not silently interchangeable features. Mono is duplicated; stereo is preserved; >2-channel sources are rejected until a documented channel-layout/downmix policy exists. Extra output channels are silent.

Seek/switch invalidates buffered playback when the worker applies the queued command. Manual gain, normalization gain and pause are callback-side atomics; selecting a file preserves the pause flag. Consecutive queued seeks/plays are coalesced, but an in-flight filesystem read cannot be cancelled. Status position is the last rendered source frame (or the pending seek target), not the decoder's ahead-of-playback position; it excludes hardware latency. `rendered_generation` distinguishes applied commands from audio actually reaching the callback. An underrun counter counts callback shortages while decoding is active, including seek/switch transition gaps, but not natural EOF.

Request a 256-frame output buffer clamped to the device's advertised range; use the host default only when the range is unknown. This changed local median command-to-rendered latency from ~43 ms to ~8 ms. The 8192-frame ring is read-ahead, not mandatory added latency: each seek changes its generation and discards stale frames.

The callback applies linked-channel normalization after the buffer and before manual gain/clamping, then updates sample-rate-aware stereo RMS/peak envelopes. This allows live normalization changes without seeking or double-applying gain to buffered samples. `PlayNormalized` starts with a known source peak; `Normalize` validates that analysis belongs to the active path. Ordinary play resets normalization. [Analysis conventions](analysis.md) define units, limits and non-goals.

## Caching and responsiveness

Waveform analysis is streaming, produces a bounded overview, and caches by canonical path, size, modification time and analysis version. It never stores decoded tracks in memory. The disk cache has a size budget and evicts oldest entries. Metadata/mtime is not a cryptographic content identity: same-size edits with deliberately preserved timestamps need cache deletion. Analysis and directory discovery are explicit CLI jobs, independent of the playback worker. The GUI uses separate scan and analysis workers, bounded result queues, generation tokens and cooperative waveform cancellation. Cancellation is checked every 4096 source frames; it cannot interrupt OS I/O. Both non-cancellable CLI and cancellable GUI entry points share the same implementation.

Optional STFT analysis uses a channel ring/FFT workspace and a bounded 512×256 overview, not a full-file sample or spectrogram matrix. Waveform and spectral caches have independent 64 MiB budgets and source/version keys; FFT size is included in spectral identity. Cancellation is checked at waveform decode intervals and every spectral hop. The same analysis powers CLI JSON and GUI views.

The audio ring provides bounded read-ahead for the current file. Speculative next-file preloading, progressive multiresolution waveforms and a persistent searchable metadata index are follow-ups. Do not call the initial CLI foundation a finished performance implementation.

## GUI responsiveness

CPAL stream ownership stays on an audio-service thread (including open, reconfigure and drop), avoiding platform-specific `Stream: Send` assumptions. Views submit typed `viewfrx_audio::Command` values over a bounded queue and read latest snapshots with `try_lock`. Engine status mutexes never reach the GUI or realtime callback.

Scan results arrive in 128-entry batches through an eight-slot queue; at most eight batches are processed per UI update. The display caps at 100,000 candidates and virtualizes rows. Filtering is incremental for incoming batches, and recomputed in memory on query edits. Workers deduplicate imported roots' results and apply alphabetical per-directory traversal. Selection has its own generation and analysis worker, so it does not wait behind a whole-library scan. Old metadata/waveforms/spectral images cannot overwrite newer selections or display settings. Normalized audition is held as a cancellable UI intent until a source peak exists; pending seek targets are preserved and Stop/device/selection changes cancel the intent. The UI performs no scanning, source-file I/O, decoding or device opening.

Closing the GUI signals worker cancellation without joining potentially blocked OS reads on the event thread. Catalog/analysis writes use the existing atomic-write path; don't rely on an import being persisted if the process exits before that job completes. See [GUI notes](gui.md) for test boundaries and remaining work.

## Platform and licensing policy

Linux is tested locally. CPAL supports Windows/macOS, but builds and device behavior must be verified on those systems. No remote or hosted CI is configured yet. Cargo.lock pins the actual dependency graph. Primary dependencies: Symphonia MPL-2.0; CPAL Apache-2.0; rtrb MIT/Apache-2.0; Clap MIT/Apache-2.0; Serde MIT/Apache-2.0; egui/eframe MIT/Apache-2.0; rfd MIT; image MIT/Apache-2.0; optional RustFFT MIT/Apache-2.0. Keep library notices for distribution and review the complete transitive graph before release. No application license is selected on the owner's behalf.
