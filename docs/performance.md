# Performance baseline

Local run, 2026-11-05, Rust 1.95 release build, Linux x86_64, default CPAL/ALSA output through the local PipeWire/PulseAudio setup. Raw observations: [performance-baseline.json](performance-baseline.json). This is a small smoke baseline, not a statistically robust benchmark campaign.

## Results

| Workload | Observation |
|---|---:|
| List 10,000 extension-filtered paths, JSON output captured | ~17 ms |
| Decode/render 60 s stereo PCM WAV, 48 kHz | ~48 ms (~1244× realtime) |
| Decode/render 600 s stereo PCM WAV, 48 kHz | ~446 ms (~1347× realtime) |
| Decode/render 60 s MP3 | ~67 ms (~896× realtime) |
| 600 s waveform, empty analysis cache | ~390 ms |
| Same waveform, populated analysis cache | ~7 ms |
| Peak process RSS, 60 s / 600 s WAV benchmark | 4464 / 4596 KiB |
| File switch, command → first rendered generation, n=5 | median 7.7 ms; range 4.5–11.0 ms |
| Seek, command → first rendered generation, n=5 | median 5.6 ms; range 2.3–8.0 ms |
| Steady playback observation, 1 second | 0 additional underrun callbacks |

The hardware test was **muted** (gain zero). It verified the stream callbacks, controls, MP3 seeking, natural EOF and error recovery, not subjective sound quality. Eight callback shortages were counted across startup/control transitions; do not describe this as a completely underrun-free session. The counter includes intervals after invalidating old audio and before new audio becomes available.

The 10-minute WAV is ~115 MB; similar process RSS for one/ten minutes is evidence of streaming, not a formal upper bound for every codec.

## Latency experiment

Using the host's default buffer produced median switch/seek latencies near 43 ms, with individual values around 85 ms. Changing only the requested device buffer to 256 frames, clamped to the advertised supported range, brought medians below 10 ms. A unit test locks down range clamping and the unknown-range fallback. The engine still has 8192 frames of producer read-ahead; generation invalidation prevents that backlog from adding seek latency.

The remaining transition gap suggests prefilling the new generation before declaring it active and refining wakeup scheduling. It must be investigated without simply suppressing shortage telemetry. Smooth de-click envelopes and bandlimited resampling are also needed before claiming high-quality auditioning.

## Level/spectrogram follow-up

[Feature-enabled raw observations](analysis-baseline.json), same local release environment:

| Workload | Observation |
|---|---:|
| 600 s stereo STFT overview, empty analysis cache | ~1257 ms |
| Same STFT overview, populated cache | ~25.5 ms |
| Stored overview | 440 time columns × 256 frequency bands |
| Muted switch / seek medians | ~7.75 / ~5.55 ms |
| Steady one-second observation | 0 additional shortages |

The updated device script also verifies live normalization, normalized start, restoration of ordinary gain and preservation of mute. Its fixture peak is −26.23 dBFS; computed auto gain is +25.23 dB, targeting −1 dBFS before manual gain. Nonzero output/meter correctness is covered by offline callback tests, not an audible hardware test. Transition shortages still occur; the steady-state result is not a whole-session claim.

[Native spectral GUI run](spectrogram-gui-baseline.json): 10,000 candidates, completed waveform/spectrogram, maximum measured update CPU ~0.86 ms. This excludes GPU/display latency and startup/capture/teardown; see [GUI verification](gui.md). The separate chirp/burst demo uses actual decoded samples and is recorded in [analysis-demo-baseline.json](analysis-demo-baseline.json).

An additional [uncached 10-minute WAV GUI run](long-analysis-gui-baseline.json) completed waveform and spectral analysis while rendering 297 updates during a five-second hold. Maximum measured update CPU was ~1.31 ms. This exercises background analysis of a longer recording, but remains a warm-filesystem smoke test rather than a sustained-load guarantee.

## Resource metrics (build size, RAM, latency, backend)

Measured 2026-11-07, Rust 1.95 release build (`--workspace --features spectrogram`), Linux x86_64:

| Metric | Observation |
|---|---:|
| CLI binary (`viewfrx`) | ~5.0 MiB |
| GUI binary (`viewfrx-gui`, egui/eframe + glow) | ~16 MiB |
| Peak RSS, decode 600 s stereo WAV (110 MiB file) | ~5.6 MiB |
| Peak RSS, waveform analysis of the same file | ~5.5 MiB |
| Peak RSS, spectrogram analysis of the same file | ~10 MiB |
| Decode throughput, 600 s WAV | ~1100× realtime |
| Output buffer requested | 256 frames (~5.3 ms at 48 kHz); command→rendered median ~8 ms |

RAM safety on huge imports: discovery streams directory entries in batches (never loads file contents); decoding is packet-bounded; the playback ring is a fixed 8192 frames; waveform/spectrogram overviews are bounded (≤65536 bins, ≤512×256 columns) and their disk caches have 64 MiB budgets with oldest-first eviction; the GUI caps displayed candidates at 100,000 entries. Importing several GB of recordings therefore costs only path strings plus bounded caches — no RAM deadlock. `scripts/check-performance.py` now reports `binary_bytes`, `gui_binary_bytes` and `peak_rss_kib` per workload alongside the existing timings.

Audio backend: **CPAL** (Apache-2.0) — actively maintained, cross-platform (ALSA/Pulse on Linux, CoreAudio/WASAPI elsewhere), actively developed at the time of writing, not end-of-life. Higher-level wrappers (rodio) hide the device/stream control this app needs; C backends (PortAudio) are effectively unmaintained. See [references.md](references.md).

## Reproduce

```sh
cargo build --release --workspace --features spectrogram --locked
python3 scripts/check-performance.py --spectrogram
python3 scripts/check-performance.py --audio --spectrogram
# Select another output if desired:
python3 scripts/check-performance.py --audio --device 1
```

The script generates fixtures in a temporary directory and deletes them on exit. It checks gapless MP3 duration when FFmpeg is available and records memory with `/usr/bin/time` when present. The optional real-device test is POSIX-only; standard Rust tests are hardware-independent.

Filesystem pages are warm from fixture creation: **“cold” above means an empty viewfrx analysis cache, not a cold OS cache**. Process startup and JSON serialization are included in list/waveform wall time. Audio latency includes JSON transport and ~1 ms status polling; it excludes hardware/DAC latency. Device open time is not included in switch/seek timing. No CPU-load stress, external drive, network drive, Windows, or macOS measurements have been made yet.

Future acceptance should use repeated p50/p95/p99 measurements over diverse real files, background CPU/I/O load, device loss, and explicit audible/loopback latency measurements—not a single hardcoded timing threshold in unit tests.
