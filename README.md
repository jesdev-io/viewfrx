# frxview

A compact, CLI-first Rust audio browser with a **native desktop GUI**.

Browse local recordings in place, inspect metadata, audition WAV/MP3, seek, change gain/rate, and generate cached waveform overviews. Source audio is never modified. Both interfaces use the same core and audio APIs.

## Build

Rust 1.89+ is declared by the dependency graph; **locally verified with Rust 1.95 on Linux**. Windows/macOS support is intended through CPAL, but not yet built or runtime-tested.

```sh
# Debian/Ubuntu native build dependencies
sudo apt install build-essential pkg-config libasound2-dev
cargo build --release --locked
./target/release/frxview --help
```

The CLI binary is `target/release/frxview` (`frxview.exe` on Windows). Plain `cargo build`/`cargo run` still target the CLI only; build the whole workspace or select the GUI explicitly. Python and FFmpeg are optional test tools, not application runtime dependencies.

## Desktop app

```sh
cargo run --release -p frx-gui -- /path/to/recordings
# Or build both binaries:
cargo build --release --workspace --locked
./target/release/frxview-gui
```

- **Left:** virtualized, filterable file list, recursive browsing and optional audition-on-selection. Use **+ Files**, **+ Folder**, drag-and-drop, or startup paths to import. First selection on startup does not auto-play.
- **Center:** technical/embedded metadata, a cached waveform, click/drag seeking, position and transport controls.
- **Right:** output-device selection/retry, gain, pitch-changing playback rate and a live output peak meter.
- **Keys:** Space play/pause; Up/Down browse visible results; Left/Right seek five seconds; Esc stop. Shortcuts do not intercept text entry.
- **JESDEV theme:** dark compact panels, amber waveform/actions, purple playhead, bundled logo with provenance.

The GUI shares the CLI's catalog/cache paths (`--library` and `--cache` override them). It deduplicates overlapping roots and caps displayed candidates at 100,000, with an explicit warning. Device switches stop playback. Output failure does not prevent browsing or analysis. `--muted` starts at gain zero.

On Linux the GUI uses egui/eframe with OpenGL and X11/Wayland runtime libraries; file dialogs use the desktop portal (`xdg-desktop-portal` plus a desktop-specific backend). It compiled here without additional GUI development headers. Drag-and-drop and startup paths remain alternatives if a desktop portal is unavailable. Windows/macOS native builds and dialogs remain unverified.

See [GUI design, tests and limitations](docs/gui.md).

## Quick start

```sh
# Import stores canonical source paths, NOT copies of the recordings.
./target/release/frxview import /path/to/recordings
./target/release/frxview list --recursive

# You can also browse/play without importing first.
./target/release/frxview list /path/to/recordings --recursive
./target/release/frxview inspect '/path/to/recording.wav'
./target/release/frxview devices
./target/release/frxview play '/path/to/recording.wav' --gain 0.5
./target/release/frxview play '/path/to/recording.mp3' --rate 1.5 --seconds 10
./target/release/frxview waveform '/path/to/recording.wav'
```

- Results are JSON; `list` emits one JSON object per file. Application errors go to stderr with a nonzero exit code; Clap usage errors use its standard text output.
- Listing filters by extension; `inspect` and `play` validate the codec/container. Nested directories require `--recursive`; `--sort` enables alphabetical order within each directory. Directory symlinks are not followed. Overlapping imported roots can yield duplicate listing entries.
- Catalog defaults to `.frxview/library.json` in the working directory; override with global `--library PATH`. Import is atomic, but catalog writes assume one writer at a time.
- Waveforms default to `.frxview/waveforms`; override with `waveform --cache DIR`. `--bins` accepts powers of two from 2 to 65536. The cache budget is 64 MiB and eviction only touches frxview-owned entries.
- Gain is linear, 0–4; 1 is unity. Output is clamped to [-1, 1] and clipped samples are counted. No automatic loudness normalization.
- Rate is 0.25–4 and **changes pitch**. Linear interpolation is a preview implementation, not antialiased resampling or pitch-preserving time stretching.
- Use `--device INDEX` on `play` or `session` to choose an output. Indices come from `devices` and may change when hardware changes. The device's default sample rate/channel count is used; a small supported output buffer is requested.

## Persistent control session

```sh
./target/release/frxview session
```

Reads one JSON object per line from stdin and replies with one JSON object per line on stdout. Paths containing spaces need only normal JSON quoting. Send these interactively or from a process controlling stdin/stdout:

```json
{"op":"play","path":"/path/to/recording.wav"}
{"op":"status"}
{"op":"seek","seconds":12.5}
{"op":"rate","value":1.5}
{"op":"gain","value":0.5}
{"op":"pause"}
{"op":"resume"}
{"op":"stop"}
{"op":"quit"}
```

The initial reply is `{"event":"ready","protocol":1}`. Transport commands acknowledge **acceptance, not completion**. Poll `status` to observe errors, position, and state. `generation` changes as the worker applies play/seek/rate/stop; `rendered_generation` changes when frames from that generation reach the output callback. It is not DAC/audible latency. Consecutive queued play/seek commands may coalesce. Pause and gain apply immediately; changing selection preserves pause.

A missing/corrupt file produces an asynchronous error in status; a later successful play clears it. Device stream failures require reopening the session. Invalid commands produce an error reply without ending the session. Requests are limited to 64 KiB. EOF or `quit` stops the engine; **piping play then immediately quit does not wait for playback**. Use the one-shot `play` command for that.

## Architecture

| Crate | Responsibility |
|---|---|
| `frx-core` | Catalog/discovery, Symphonia decoding and metadata, waveform cache |
| `frx-audio` | Typed transport, deterministic renderer, decoder worker, CPAL callback |
| `frx-cli` | Clap and JSON-lines adapters |
| `frx-gui` | egui views, background adapters, generation-aware browser/analysis state |

Audio streams through an 8192-frame SPSC ring. Decoding, resampling, file opening, and seeking run on a worker; the callback does no disk I/O, decoding, allocation, logging, or mutex locking. Old-generation frames are discarded after seek/switch. Whole recordings are not loaded into memory.

See [architecture](docs/architecture.md), [references and branding](docs/references.md), and [remaining work](docs/roadmap.md). `goal.md` is the original, unmodified brief.

## Test and measure

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked
./target/release/frxview benchmark '/path/to/recording.wav'
python3 scripts/check-performance.py
python3 scripts/check-performance.py --audio  # MUTED real-device smoke test
cargo build --release -p frx-gui --locked
python3 scripts/check-gui.py --screenshot /tmp/frxview.png  # requires a display
```

Unit/integration tests do not require an audio device. WAV fixtures are generated in temporary directories; the tiny synthetic MP3 fixture is committed with its generation command. The performance script generates 60-second and 600-second WAVs and 10,000 directory entries, then removes them. FFmpeg is optional and enables an additional generated MP3 benchmark. The optional device test uses POSIX pipe polling, gain zero, and never deliberately plays the synthetic tone audibly.

[Measured baseline and caveats](docs/performance.md). These tests establish a foundation—not proof that every codec, driver, or slow filesystem is glitch-free.

## Current limitations

- Spectrogram, spectrum, VU ballistics, annotation editing, waveform zoom, and persisted playback preferences are not implemented. Status exposes a callback peak, not a calibrated VU meter.
- Playback supports mono/stereo; multichannel metadata/waveforms work, but auditioning >2 channels is rejected. Extra device channels are silent.
- WAV and MP3 are tested. Other Symphonia-enabled formats may work but are not yet qualified. A truncated stream ending at a valid packet boundary may look like normal EOF; this is not an audio-file integrity checker.
- Current-file read-ahead and a bounded overview cache exist. Next-file preloading and progressive multiresolution waveforms remain work.
- GUI discovery/analysis run on separate workers with cooperative cancellation and stale-result rejection. OS file reads, directory enumeration/sorting and device opening cannot be interrupted; a newer request may wait for one already in progress, but not on the UI thread. CLI analysis remains an explicit job.
- Cache identity uses path/size/mtime, not a full content hash. If files change while preserving both size and mtime, delete their cached analyses.
- Cross-platform release packaging, dependency/security audits, and application licensing still need decisions. No remote is configured.
