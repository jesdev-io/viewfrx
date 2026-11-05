# frxview

A compact, CLI-first Rust audio browser. **Initial headless milestone; no GUI yet.**

Browse local recordings in place, inspect metadata, audition WAV/MP3, seek, change gain/rate, and generate cached waveform overviews. Source audio is never modified. The future desktop interface will use the same Rust APIs.

## Build

Rust 1.88+ is declared by the dependency graph; **locally verified with Rust 1.95 on Linux**. Windows/macOS support is intended through CPAL, but not yet built or runtime-tested.

```sh
# Debian/Ubuntu native build dependencies
sudo apt install build-essential pkg-config libasound2-dev
cargo build --release --locked
./target/release/frxview --help
```

The binary is `target/release/frxview` (`frxview.exe` on Windows). No GUI toolkit, Python, FFmpeg, or network service is needed at runtime.

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
- Listing filters by extension; `inspect` and `play` validate the codec/container. Nested directories require `--recursive`. Directory symlinks are not followed. Overlapping imported roots can yield duplicate listing entries.
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
```

Unit/integration tests do not require an audio device. WAV fixtures are generated in temporary directories; the tiny synthetic MP3 fixture is committed with its generation command. The performance script generates 60-second and 600-second WAVs and 10,000 directory entries, then removes them. FFmpeg is optional and enables an additional generated MP3 benchmark. The optional device test uses POSIX pipe polling, gain zero, and never deliberately plays the synthetic tone audibly.

[Measured baseline and caveats](docs/performance.md). These tests establish a foundation—not proof that every codec, driver, or slow filesystem is glitch-free.

## Current limitations

- GUI, spectrogram, spectrum, VU ballistics, annotation editing, and persisted playback preferences are not implemented. Status exposes a callback peak, not a calibrated VU meter.
- Playback supports mono/stereo; multichannel metadata/waveforms work, but auditioning >2 channels is rejected. Extra device channels are silent.
- WAV and MP3 are tested. Other Symphonia-enabled formats may work but are not yet qualified. A truncated stream ending at a valid packet boundary may look like normal EOF; this is not an audio-file integrity checker.
- Current-file read-ahead and a bounded overview cache exist. Next-file preloading, progressive multiresolution waveforms, and cancellable analysis/indexing remain work.
- Directory discovery/analysis are explicit CLI jobs. A GUI must schedule them off its event thread. In-flight filesystem I/O can delay queued controls or shutdown, especially on network drives.
- Cache identity uses path/size/mtime, not a full content hash. If files change while preserving both size and mtime, delete their cached analyses.
- Cross-platform release packaging, dependency/security audits, and application licensing still need decisions. No remote is configured.
