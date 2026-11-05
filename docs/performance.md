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

## Reproduce

```sh
cargo build --release --locked
python3 scripts/check-performance.py
python3 scripts/check-performance.py --audio
# Select another output if desired:
python3 scripts/check-performance.py --audio --device 1
```

The script generates fixtures in a temporary directory and deletes them on exit. It checks gapless MP3 duration when FFmpeg is available and records memory with `/usr/bin/time` when present. The optional real-device test is POSIX-only; standard Rust tests are hardware-independent.

Filesystem pages are warm from fixture creation: **“cold” above means an empty frxview analysis cache, not a cold OS cache**. Process startup and JSON serialization are included in list/waveform wall time. Audio latency includes JSON transport and ~1 ms status polling; it excludes hardware/DAC latency. Device open time is not included in switch/seek timing. No CPU-load stress, external drive, network drive, Windows, or macOS measurements have been made yet.

Future acceptance should use repeated p50/p95/p99 measurements over diverse real files, background CPU/I/O load, device loss, and explicit audible/loopback latency measurements—not a single hardcoded timing threshold in unit tests.
