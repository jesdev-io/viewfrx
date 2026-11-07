# Levels, normalization and optional spectrogram

## Level conventions

- Relative gain: `20 log10(gain)` dB; 0 dB = unity. The GUI edits dB, while the existing session `gain` command remains linear. Double-clicking the gain slider or its value restores 0 dB and unmutes.
- Signal amplitude: `20 log10(abs(amplitude))` dBFS, referenced to unit digital amplitude. Zero is `null` in status JSON and `−∞` in the GUI.
- The vertical L/R meters measure output **after normalization, manual gain and clamp**, before device-format quantization/DAC. Mono is duplicated; a mono output device is metered as the actual downmix.
- RMS uses a sample-rate-aware exponential power envelope with a 300 ms time constant. A stationary full-scale sine approaches −3.01 dBFS RMS under this unit-amplitude convention. Peak markers attack immediately and decay at 20 dB/s. Silence, pause, stop and shortages feed zeros through the envelopes.
- This is a VU-style RMS/peak display, not an IEC-calibrated electromechanical VU instrument, loudness meter, or intersample true-peak meter. The legacy callback `peak` status field remains available for compatibility.

## Normalized waveform and playback

The amber waveform is the original source. The subdued waveform behind it uses

```
p = maximum absolute decoded sample, across all source channels
normalization_gain = 10^(-1 / 20) / p
```

The cache's min/max envelope preserves this sample peak; it is not estimated from rendered pixels. Both the preview and transport use the same `viewfrx-core` level helper. Silence uses gain 1; invalid/non-finite gain is rejected. Linked channels preserve stereo balance.

**Normalize** applies that gain before the independent manual gain, without modifying the recording. At manual 0 dB, the source sample peak targets −1 dBFS. Additional manual gain can still clip. This matches Audacity's linked-channel peak-normalization behavior, not its optional DC-offset removal, independent-channel normalization, loudness processing or true-peak limiting.

When analysis is already available, playback starts with normalization atomically and toggling it does not seek. If analysis is missing, normalized playback waits on the background worker—never briefly plays at an unknown gain. The GUI remains interactive; **Cancel**, Stop, a different selection, and output changes cancel the pending audition. A seek made while waiting is retained. Disabling Normalize releases the wait and permits ordinary playback. Long, uncached files therefore have a first-normalized-play analysis cost; normal auditioning remains immediate/streamed. Cache hits avoid rescanning the recording.

CLI equivalents:

```sh
viewfrx normalization recording.wav
viewfrx play recording.wav --normalize --gain-db -6
```

Sessions accept `play_normalized {path, peak}` and `normalize {path, peak}` operations. Supply the measured source peak from `normalization`; `normalize` with `peak: null` disables it. The path guard rejects results belonging to another current recording. Ordinary `play` resets normalization, while seeking/rate changes preserve it. This is not file-change monitoring: external edits require a new analysis/selection.


## DC offset, anti-pop fades and the fixed highpass

- Analysis reports the per-channel DC offset as a file parameter, in linear amplitude (`waveform.dc_offset`, `levels.dc_offset`) and in dBFS (`dc_offset_dbfs`, `null` for zero DC). Normalization targets the **DC-removed** sample peak, so a DC offset never steers the auto gain; the raw peak is still reported as `sample_peak`.
- Playback always removes DC with a one-pole DC blocker (5 Hz, `viewfrx-audio` config). A short 10 ms onset/offset gain fade is applied to the first/last source samples (and after every seek), so recording start/stop and jumps never produce a dirac-pop. The optional fixed highpass (20 Hz, `play --highpass`, session `highpass`, GUI toggle) additionally removes infra-sound. Both filters are preview-grade one-pole sections in the renderer, not room-correction DSP.
- The GUI waveform plots the DC-removed signal (what you hear); annotation markers are drawn as vertical lines.

## Magnitude spectrum

`viewfrx spectrum recording.wav --fft-size 2048 --position 12.5` (feature `spectrogram`) computes one Hann-windowed magnitude spectrum in amplitude dBFS at a position — the same calibrated, channel-power-averaged STFT as the spectrogram, but a single window. The JSON also carries `third_octave_hz`, the ISO 1/3-octave center frequencies up to Nyquist that the GUI draws as its spectrum grid, and `--third-octave` aggregates the spectrum into those log-frequency bands (max dBFS per band) for CLI parity with the GUI's log-frequency view. The GUI spectrum panel uses a log-frequency axis, a height-dependent dB grid step, and plots the highpass response when enabled. `waveform`/`spectrogram --from/--to` emit time-windowed overviews (the GUI zoom equivalent).

## Timestamp annotations

- Free text with `[hh:mm:ss:cc]` markers (dd = days) becomes annotations: `viewfrx annotate FILE "[00:00:12:00] thunder"`, or `--seconds 42 "note"` for a position marker (GUI: right-click a plot). Times canonicalize to whole seconds, matching the marker format.
- Storage is the file's own metadata, written atomically (temp + rename) without touching audio data: WAV gets a `LIST`/`INFO` chunk with a custom `FRXA` sub-chunk inserted before `data` (where players and Symphonia read it); MP3 gets an ID3v2 `TXXX` frame (`viewfrx-annotations`). All other chunks/frames are preserved, so the file stays a normal playable audio file. Other formats are rejected with an error and left unmodified.
- Merging is idempotent: identical (time, text) pairs are never duplicated, so text-field entry and right-click markers render each other. `viewfrx annotations FILE` (and the session op) lists them; the GUI shows them as waveform lines and in the bottom field.
- In the GUI, new markers are floating (in memory) until saved with Ctrl+S / File → Save annotations, which opens a standard save dialog (file naming, native overwrite warning) and writes atomically via `annotations::save_as`; switching files with unsaved changes warns first (Save / Dismiss / Cancel). On the CLI, `annotate` writes in place and `annotate --output DST` saves an annotated copy, leaving the source untouched.

## Spectrogram

```sh
cargo build --release --workspace --features spectrogram --locked
./target/release/viewfrx spectrogram recording.wav --fft-size 2048
./target/release/viewviewfrx-gui /path/to/recordings
```

Feature builds can show the source spectrogram; it starts off by default (settings toggle or `--spectrogram`). FFT sizes are 512, 1024, 2048, 4096 or 8192; default 2048. Display floor defaults to −100 dBFS, with 0 dBFS at the top of the color scale. Double-click controls to restore defaults. Either plot supports seeking; double-click rewinds.

The shared core implements:

1. Streaming STFT with a periodic Hann window, hop `FFT size / 4` (75% overlap), and half-window boundary zero-padding so column centers align with source time.
2. One-sided amplitude calibration by Hann coherent gain (`sum(window)`), doubling interior FFT amplitudes but not DC/Nyquist.
3. Power averaging **across channels**, then conversion with `10 log10(power)`. This preserves equal anti-phase stereo tones that a waveform downmix would cancel.
4. Maximum aggregation into at most **512 time columns × 256 frequency bands**, with streaming time compaction. It preserves transient/tonal maxima, not integrated energy or power spectral density. Increasing FFT size changes analysis resolution/leakage; the overview's display resolution remains capped.
5. Finite analysis floor −160 dBFS, linear frequency axis from DC to Nyquist, cooperative cancellation every hop, and a separate 64 MiB disk cache.

The first/last windows include zeros and can show boundary transients (including a DC step); steady-state calibration tests use full windows. The overview is not a sample-accurate zoom editor: time compaction, frequency pooling and final partial columns limit visual localization. The spectrogram always represents the **source**, unaffected by playback gain/normalization/rate.

FFT work and color conversion happen on the analysis worker. The GUI only accepts current-generation results and uploads the bounded texture. Metadata and waveform are delivered before the spectral image. No decoding/FFT work is added to the audio callback.

Cache keys include source path/size/mtime, analysis version and FFT size. With GUI `--cache DIR`, spectral files live in `DIR/spectrograms`. The CLI `spectrogram --cache DIR` instead names that spectral directory directly. Both defaults resolve to `.viewfrx/waveforms/spectrograms`. Waveform and spectral caches each have a 64 MiB budget and only evict owned entries. Display-floor changes recolor cached analysis rather than changing its numeric data.

## Configuration and verification

Production policy/tuning lives in `crates/viewfrx-{core,audio,cli,gui}/src/config.rs`; GUI colors and palette anchors live in `theme.rs`. This includes DSP defaults, cache/queue limits, polling, layout dimensions, spacing, meter ticks, control defaults and bounds. Mathematical identities, channel indices, unit conversions and explicit test-fixture values remain at their use sites. Changes affecting cached analysis semantics require a cache-version bump.

Tests cover peak normalization/silence, normalized callback output, stereo channel levels and decay, full-scale sine RMS, stale-source rejection, pending audition cancellation/seeking, double-click gain reset, calibrated FFT tones, anti-phase preservation, transient timing, short silence, mid-decode cancellation, cache invalidation, CLI commands and worker-delivered color images. Both feature-enabled and waveform-only builds are checked.

See [performance measurements](performance.md), [GUI verification](gui.md), and [source references](references.md).
