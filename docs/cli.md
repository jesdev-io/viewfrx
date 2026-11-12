# CLI reference

[Documentation index](README.md) · [Installation](installation.md)

The `viewfrx` CLI is included in release archives. Examples below use a source build; replace `./target/release/viewfrx` with the path to your extracted binary when using a release. Run `viewfrx --help` or `viewfrx COMMAND --help` for command-line options.

## Command examples

```sh
# Import validates and prints canonical roots; it does not persist a library.
./target/release/viewfrx import /path/to/recordings
./target/release/viewfrx list --recursive

# You can also browse/play without importing first.
./target/release/viewfrx list /path/to/recordings --recursive
./target/release/viewfrx inspect '/path/to/recording.wav'
./target/release/viewfrx devices
./target/release/viewfrx play '/path/to/recording.wav' --gain 0.5
./target/release/viewfrx play '/path/to/recording.mp3' --rate 1.5 --seconds 10
./target/release/viewfrx waveform '/path/to/recording.wav'
./target/release/viewfrx normalization '/path/to/recording.wav'
./target/release/viewfrx play '/path/to/recording.wav' --normalize --gain-db -6
./target/release/viewfrx play '/path/to/recording.wav' --highpass
./target/release/viewfrx spectrogram '/path/to/recording.wav' --fft-size 2048
./target/release/viewfrx levels '/path/to/recording.wav'
./target/release/viewfrx waveform '/path/to/recording.wav' --stereo
./target/release/viewfrx annotate '/path/to/recording.wav' '[00:00:12:00] thunder'
./target/release/viewfrx annotate '/path/to/recording.wav' --seconds 42 'clap'
./target/release/viewfrx annotate '/path/to/recording.wav' --output '/path/copy.wav' '[00:00:12:00] thunder'
./target/release/viewfrx annotations '/path/to/recording.wav'
./target/release/viewfrx spectrum '/path/to/recording.wav' --position 12.5
./target/release/viewfrx spectrum '/path/to/recording.wav' --third-octave  # log-frequency 1/3-octave bands
./target/release/viewfrx waveform '/path/to/recording.wav' --from 10 --to 20
```

- Results are JSON; `list` emits one JSON object per file. Application errors go to stderr with a nonzero exit code; Clap usage errors use its standard text output.
- Listing filters by extension; `inspect` and `play` validate the codec/container. Nested directories require `--recursive`; `--sort` enables alphabetical order within each directory. Directory symlinks are not followed. Overlapping imported roots can yield duplicate listing entries.
- `list` without a path browses the working directory, never a saved library. `import` only validates/emits roots; it does not retain them.
- Waveforms use an automatically removed temporary cache; explicitly retain artifacts with `waveform --cache DIR`. `--bins` accepts powers of two from 2 to 65536. The cache budget is 64 MiB and eviction only touches viewfrx-owned entries.
- Manual gain accepts linear `--gain` (0–4, 1 is unity) or `--gain-db`. Output is clamped to [-1, 1] and clipped samples are counted.
- `--normalize` applies linked-channel sample-peak gain to reach −1 dBFS **before manual gain**, computed on the **DC-removed** signal. It preserves stereo balance and leaves silence unchanged. This is not loudness/true-peak normalization. The CLI computes or loads peak analysis before starting; the GUI waits without blocking its event loop and never starts normalized playback at an unanalysed gain.
- Playback always removes DC offset with a one-pole DC blocker and applies a short onset/offset fade (anti dirac-pop). `--highpass` additionally enables a fixed 20 Hz highpass against infra-sound. `levels` reports the per-channel DC offset in linear and dBFS, raw and DC-removed peaks, and the normalization. `benchmark --highpass` applies the same chain.
- `waveform --channel N` / `--stereo` select per-channel overviews; `spectrogram --channel N` / `--stereo` do the same for spectral analysis.
- `annotate` merges `[hh:mm:ss:cc]` markers (or `--seconds`) into the file's own metadata (WAV LIST INFO / MP3 ID3v2) atomically, without altering the audio data; identical annotations are never duplicated. `--output DST` saves an annotated copy instead of modifying the source (the GUI save-dialog equivalent). `annotations` lists them. Both are also session ops. In the GUI, markers are floating until saved via Ctrl+S / the File menu; switching files with unsaved markers warns first.
- `spectrum` (feature `spectrogram`) returns one calibrated magnitude spectrum in dBFS at `--position`; `waveform`/`spectrogram` accept `--from/--to` zoom windows. Global `--verbose` pipes console log lines to stderr as JSON (headless counterpart of the GUI debug console).
- `spectrogram` requires its Cargo feature. The CLI returns the same calibrated, bounded STFT overview used by the GUI; its separate 64 MiB cache is temporary unless `--cache DIR` is supplied.
- Rate is 0.25–4 and **changes pitch**. Linear interpolation is a preview implementation, not antialiased resampling or pitch-preserving time stretching.
- Use `--device INDEX` on `play` or `session` to choose an output. Indices come from `devices` and may change when hardware changes. The device's default sample rate/channel count is used; a small supported output buffer is requested.

## Persistent control session

```sh
./target/release/viewfrx session
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
{"op":"highpass","enabled":true}
{"op":"annotate","path":"/path/to/recording.wav","text":"[00:00:12:00] thunder"}
{"op":"annotations","path":"/path/to/recording.wav"}
{"op":"quit"}
```

The initial reply is `{"event":"ready","protocol":1}`. Transport commands acknowledge **acceptance, not completion**. Poll `status` to observe errors, position, and state. `generation` changes as the worker applies play/seek/rate/stop; `rendered_generation` changes when frames from that generation reach the output callback. It is not DAC/audible latency. Consecutive queued play/seek commands may coalesce. Pause and gain apply immediately; changing selection preserves pause.

Use `normalization PATH` to obtain the actual DC-removed source peak, then send `{"op":"play_normalized","path":"/path/to/recording.wav","peak":0.5}` with that measured peak (0.5 here is only an example). For the current file, `{"op":"normalize","path":"/path/to/recording.wav","peak":0.5}` changes normalization without a seek; `"peak":null` disables it. The path guard rejects stale analysis for a different recording. Ordinary `play` resets normalization. Manual/session `gain` remains linear for compatibility; status additionally reports gain in dB, normalization gain, the highpass state and per-channel RMS/peak dBFS (`null` for zero).

A missing/corrupt file produces an asynchronous error in status; a later successful play clears it. Device stream failures require reopening the session. Invalid commands produce an error reply without ending the session. Requests are limited to 64 KiB. EOF or `quit` stops the engine; **piping play then immediately quit does not wait for playback**. Use the one-shot `play` command for that.
