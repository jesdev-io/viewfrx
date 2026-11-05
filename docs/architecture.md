# Architecture and first milestone

## Boundaries

- `frx-core`: filesystem discovery, streaming decoding/probing, cached waveform analysis. No device, UI, or argument-parsing dependencies.
- `frx-audio`: typed transport commands, streaming preview engine, CPAL device adapter. No CLI or UI dependencies.
- `frx-cli`: Clap and JSON-lines adapters. A future GUI uses the same Rust API, never subprocess playback.

We deliberately start with three crates, not a generic plugin/command framework.

## Audio contract

Disk access, seeking and decoding belong on a producer thread. CPAL consumes a bounded SPSC ring; the callback never takes a mutex, reads a file, decodes, logs, or allocates. Atomics hold transport flags and telemetry. Each buffered frame carries a generation: old audio is discarded after seek/switch/stop. Command submission is bounded and reports overload rather than blocking. Worker errors are visible in status. Shutdown joins the worker (local filesystem I/O can still delay shutdown).

Rate adjustment changes pitch and duration together. The first preview renderer uses linear interpolation, not mastering-quality antialiased resampling or pitch-preserving time stretching. These are explicit follow-up DSP tasks, not silently interchangeable features. Mono is duplicated; stereo is preserved; >2-channel sources are rejected until a documented channel-layout/downmix policy exists. Extra output channels are silent.

Seek/switch invalidates buffered playback immediately. Gain/pause are callback-side atomics. Status position is the last rendered source frame, not the decoder's ahead-of-playback position; it excludes hardware latency. An underrun counter counts callback shortages while decoding is active, not natural EOF.

## Caching and responsiveness

Waveform analysis is streaming, produces a bounded overview, and caches by canonical path, size, modification time and analysis version. It never stores decoded tracks in memory. The disk cache has a size budget and evicts oldest entries. Metadata/mtime is not a cryptographic content identity: same-size edits with deliberately preserved timestamps need cache deletion. Analysis and directory discovery are explicit CLI jobs, independent of the playback worker; the future GUI must schedule them off its event thread.

The audio ring provides bounded read-ahead for the current file. Speculative next-file preloading, progressive multiresolution waveforms, background library indexing, and cancellable analysis are follow-ups. Do not call the initial CLI foundation a finished performance implementation.

## Platform and licensing policy

Linux is tested locally. CPAL supports Windows/macOS, but builds and device behavior must be verified on those systems. No remote or hosted CI is configured yet. Cargo.lock pins the actual dependency graph. Primary dependencies: Symphonia MPL-2.0; CPAL Apache-2.0; rtrb MIT/Apache-2.0; Clap MIT/Apache-2.0; Serde MIT/Apache-2.0. Keep library notices for distribution and review the complete transitive graph before release. No application license is selected on the owner's behalf.
