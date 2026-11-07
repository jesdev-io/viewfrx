mod config;
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use config::*;
use serde::Serialize;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use viewfrx_audio::{Command, Engine};
use viewfrx_core::{
    annotations::{self, Annotation},
    catalog::{Catalog, discover_ordered},
    config::MAX_SEEK_SECONDS,
    decode::inspect,
    waveform::WaveformCache,
};
use viewfrx_core::{config::DEFAULT_WAVEFORM_BINS, levels::db_to_gain};

#[derive(Parser)]
#[command(
    name = "viewfrx",
    version,
    about = "A lightning fast audio file browser for field recordings and more"
)]
struct Args {
    #[arg(long, global = true, hide = true, default_value = ".")]
    library: PathBuf,
    /// Pipe console log lines to stderr as JSON (headless counterpart of the
    /// GUI debug console window).
    #[arg(long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Action,
}

static VERBOSE: AtomicBool = AtomicBool::new(false);
/// Console logging: JSON lines on stderr when --verbose is set.
fn vlog(message: String) {
    if VERBOSE.load(Ordering::Relaxed) {
        eprintln!("{}", json!({"log": message}));
    }
}

#[derive(Subcommand)]
enum Action {
    /// Validate session roots and emit them; does not persist a catalog.
    Import {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Stream candidate paths as JSON lines. Extension-filtered, not codec-validated.
    List {
        path: Option<PathBuf>,
        #[arg(short, long)]
        recursive: bool,
        /// Alphabetical order within each directory (may delay first result).
        #[arg(long)]
        sort: bool,
    },
    /// Probe technical metadata and embedded tags without decoding the entire file.
    Inspect { path: PathBuf },
    /// Produce a bounded min/max overview, with an on-disk cache.
    Waveform {
        path: PathBuf,
        #[arg(long, default_value_t = DEFAULT_WAVEFORM_BINS)]
        bins: usize,
        #[arg(long, default_value = WAVEFORM_CACHE)]
        cache: PathBuf,
        /// Select a source channel (0 = Left, 1 = Right).
        #[arg(long, conflicts_with = "stereo")]
        channel: Option<usize>,
        /// Return separate Left and Right overviews (stereo sources only).
        #[arg(long)]
        stereo: bool,
        /// Zoom window start in seconds (0 = file start).
        #[arg(long)]
        from: Option<f64>,
        /// Zoom window end in seconds (defaults to the file end).
        #[arg(long)]
        to: Option<f64>,
    },
    /// Calculate linked-channel peak normalization to -1 dBFS without changing audio.
    /// The gain targets the DC-removed sample peak; DC offset is not removed from the file.
    Normalization {
        path: PathBuf,
        #[arg(long, default_value = WAVEFORM_CACHE)]
        cache: PathBuf,
    },
    /// File signal parameters: DC offset in linear and dBFS, peaks, and the
    /// DC-removed normalization actually used for playback.
    Levels {
        path: PathBuf,
        #[arg(long, default_value = WAVEFORM_CACHE)]
        cache: PathBuf,
    },
    /// Streaming, bounded, cached STFT overview in amplitude dBFS.
    #[cfg(feature = "spectrogram")]
    Spectrogram {
        path: PathBuf,
        #[arg(long, default_value_t = viewfrx_core::config::spectral::DEFAULT_FFT_SIZE)]
        fft_size: usize,
        #[arg(long, default_value = SPECTROGRAM_CACHE)]
        cache: PathBuf,
        /// Select a source channel (0 = Left, 1 = Right).
        #[arg(long, conflicts_with = "stereo")]
        channel: Option<usize>,
        /// Return separate Left and Right analyses (stereo sources only).
        #[arg(long)]
        stereo: bool,
        /// Zoom window start in seconds (0 = file start).
        #[arg(long)]
        from: Option<f64>,
        /// Zoom window end in seconds (defaults to the file end).
        #[arg(long)]
        to: Option<f64>,
    },
    /// Single-window magnitude spectrum in dBFS at a position (live view source).
    #[cfg(feature = "spectrogram")]
    Spectrum {
        path: PathBuf,
        #[arg(long, default_value_t = viewfrx_core::config::spectral::DEFAULT_FFT_SIZE)]
        fft_size: usize,
        /// Window start in seconds; clamped to the recording.
        #[arg(long)]
        position: Option<f64>,
        /// Aggregate into log-frequency 1/3-octave bands (max dBFS per band).
        #[arg(long)]
        third_octave: bool,
    },
    /// Add timestamp annotations to a file's own metadata (WAV/MP3), without
    /// altering the audio. Idempotent: identical annotations are not duplicated.
    Annotate {
        path: PathBuf,
        /// Free text containing [hh:mm:ss:cc] markers, e.g. "[00:00:12:00] thunder".
        text: Option<String>,
        /// Place the annotation at this time (seconds) instead of parsing markers.
        #[arg(long)]
        seconds: Option<f64>,
        /// Save the annotated copy to this path instead of modifying the file
        /// in place (the GUI save-dialog equivalent).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Remove all annotations from the file's metadata.
        #[arg(long)]
        clear: bool,
    },
    /// List the timestamp annotations stored in a file's metadata.
    Annotations { path: PathBuf },
    /// List current output devices (indices are session-local, not persistent IDs).
    Devices,
    /// Play one file until EOF (or --seconds). Rate changes pitch.
    Play {
        path: PathBuf,
        #[arg(long)]
        device: Option<usize>,
        #[arg(long, default_value_t = 1.0)]
        gain: f32,
        /// Relative gain in dB (not dBFS); alternative to --gain.
        #[arg(long, allow_hyphen_values = true, conflicts_with = "gain")]
        gain_db: Option<f32>,
        /// Linked-channel sample-peak normalization to -1 dBFS, before manual gain.
        #[arg(long)]
        normalize: bool,
        #[arg(long, default_value = WAVEFORM_CACHE)]
        cache: PathBuf,
        #[arg(long, default_value_t = 1.0)]
        rate: f64,
        /// Fixed highpass to remove infra-sound content (DC blocker is always on).
        #[arg(long)]
        highpass: bool,
        #[arg(long)]
        seconds: Option<f64>,
    },
    /// Persistent JSON-lines control session. See README for commands.
    Session {
        #[arg(long)]
        device: Option<usize>,
    },
    /// Decode/resample without a device; report throughput and signal checksums.
    Benchmark {
        path: PathBuf,
        #[arg(long, default_value_t = BENCHMARK_SAMPLE_RATE)]
        sample_rate: u32,
        #[arg(long, default_value_t = 1.0)]
        rate: f64,
        /// Apply the fixed infra-sound highpass like playback does.
        #[arg(long)]
        highpass: bool,
    },
}

fn emit(value: &impl Serialize) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer(&mut out, value)?;
    writeln!(out)?;
    out.flush()?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{}", json!({"ok":false,"error":format!("{error:#}")}));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    VERBOSE.store(args.verbose, Ordering::Relaxed);
    vlog(format!(
        "viewfrx v{} invoked: {}",
        env!("CARGO_PKG_VERSION"),
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    ));
    let temporary_cache = tempfile::tempdir()?;
    let cache_path = |path: PathBuf| {
        if path == std::path::Path::new(WAVEFORM_CACHE) {
            temporary_cache.path().to_path_buf()
        } else {
            path
        }
    };
    match args.command {
        Action::Import { paths } => {
            let mut catalog = Catalog::default();
            catalog.import(&paths)?;
            emit(&catalog)?;
        }
        Action::List {
            path,
            recursive,
            sort,
        } => {
            let roots = if let Some(path) = path {
                vec![path]
            } else {
                vec![std::env::current_dir()?]
            };
            let mut errors = 0;
            for root in roots {
                let result = discover_ordered(&root, recursive, sort, |entry| match entry {
                    Ok(path) => emit(&json!({"path":path})),
                    Err(error) => {
                        errors += 1;
                        emit(&json!({"error":error.to_string()}))
                    }
                });
                if let Err(error) = result {
                    errors += 1;
                    emit(&json!({"path":root,"error":error.to_string()}))?;
                }
            }
            if errors > 0 {
                bail!("discovery encountered {errors} errors");
            }
        }
        Action::Inspect { path } => emit(&inspect(&path)?)?,
        Action::Waveform {
            path,
            bins,
            cache,
            channel,
            stereo,
            from,
            to,
        } => {
            let cached = WaveformCache::new(cache_path(cache)).get(&path, bins)?;
            let waveform = &cached.waveform;
            let window = match (from, to) {
                (None, None) => None,
                (from, to) => {
                    let (from, to) = viewfrx_core::waveform::validate_window(
                        from.unwrap_or(0.0),
                        to.unwrap_or(f64::INFINITY),
                        waveform.duration_seconds(),
                    )?;
                    Some(
                        serde_json::json!({"from": from, "to": to, "duration": waveform.duration_seconds()}),
                    )
                }
            };
            let windowed =
                |w: &viewfrx_core::waveform::Waveform| -> Result<viewfrx_core::waveform::Waveform> {
                    match (from, to) {
                        (None, None) => Ok(w.clone()),
                        _ => w
                            .windowed(from.unwrap_or(0.0), to.unwrap_or(f64::INFINITY))
                            .context("invalid zoom window"),
                    }
                };
            if stereo {
                if waveform.channels() != 2 {
                    bail!("--stereo requires a stereo recording");
                }
                let channels = (0..2)
                    .map(|ch| {
                        Ok(match (from, to) {
                            (None, None) => {
                                waveform.channel_waveform(ch).context("missing channel")?
                            }
                            _ => waveform
                                .channel_windowed(
                                    ch,
                                    from.unwrap_or(0.0),
                                    to.unwrap_or(f64::INFINITY),
                                )
                                .with_context(|| format!("source channel {ch} does not exist"))?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                emit(
                    &serde_json::json!({"cache_hit": cached.cache_hit, "window": window, "channels": channels}),
                )?;
            } else if let Some(ch) = channel {
                let view = match (from, to) {
                    (None, None) => waveform
                        .channel_waveform(ch)
                        .with_context(|| format!("source channel {ch} does not exist"))?,
                    _ => waveform
                        .channel_windowed(ch, from.unwrap_or(0.0), to.unwrap_or(f64::INFINITY))
                        .with_context(|| format!("source channel {ch} does not exist"))?,
                };
                emit(
                    &serde_json::json!({"cache_hit": cached.cache_hit, "window": window, "waveform": view}),
                )?;
            } else if window.is_some() {
                emit(
                    &serde_json::json!({"cache_hit": cached.cache_hit, "window": window, "waveform": windowed(waveform)?}),
                )?;
            } else {
                emit(&cached)?;
            }
        }
        Action::Normalization { path, cache } => emit(
            &WaveformCache::new(cache_path(cache))
                .get(&path, DEFAULT_WAVEFORM_BINS)?
                .waveform
                .normalization()?,
        )?,
        Action::Levels { path, cache } => {
            let cached = WaveformCache::new(cache_path(cache)).get(&path, DEFAULT_WAVEFORM_BINS)?;
            emit(&serde_json::json!({
                "cache_hit": cached.cache_hit,
                "levels": cached.waveform.levels()?,
            }))?;
        }
        #[cfg(feature = "spectrogram")]
        Action::Spectrogram {
            path,
            fft_size,
            cache,
            channel,
            stereo,
            from,
            to,
        } => {
            use viewfrx_core::spectrogram::{Settings, SpectrogramCache};
            let cache = SpectrogramCache::new(cache_path(cache));
            let apply_window = |s: viewfrx_core::spectrogram::Spectrogram,
                                cache_hit: bool|
             -> Result<()> {
                match (from, to) {
                    (None, None) => {
                        emit(&serde_json::json!({"cache_hit": cache_hit, "spectrogram": s}))
                    }
                    _ => {
                        let windowed = s
                            .windowed(from.unwrap_or(0.0), to.unwrap_or(f64::INFINITY))
                            .context("invalid zoom window")?;
                        let duration = windowed.duration_seconds();
                        emit(&serde_json::json!({"cache_hit": cache_hit,
                            "window": {"from": from.unwrap_or(0.0), "to": to.unwrap_or(duration), "duration": duration},
                            "spectrogram": windowed}))
                    }
                }
            };
            if stereo {
                if inspect(&path)?.channels != 2 {
                    bail!("--stereo requires a stereo recording");
                }
                let channels = (0..2)
                    .map(|ch| cache.get_channel(&path, Settings { fft_size }, Some(ch), || false))
                    .collect::<Result<Vec<_>>>()?;
                let channels = channels
                    .into_iter()
                    .map(|c| -> Result<viewfrx_core::spectrogram::Spectrogram> {
                        match (from, to) {
                            (None, None) => Ok(c.spectrogram),
                            _ => c
                                .spectrogram
                                .windowed(from.unwrap_or(0.0), to.unwrap_or(f64::INFINITY))
                                .context("invalid zoom window"),
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                emit(&serde_json::json!({"channels": channels}))?;
            } else {
                let cached = cache.get_channel(&path, Settings { fft_size }, channel, || false)?;
                apply_window(cached.spectrogram, cached.cache_hit)?;
            }
        }
        #[cfg(feature = "spectrogram")]
        Action::Spectrum {
            path,
            fft_size,
            position,
            third_octave,
        } => {
            use viewfrx_core::spectrum;
            let settings = viewfrx_core::spectrogram::Settings { fft_size };
            let result = spectrum::analyze_at(&path, settings, position.unwrap_or(0.0), || false)?;
            if third_octave {
                let bands = spectrum::third_octave_bands(&result);
                emit(&serde_json::json!({
                    "sample_rate": result.sample_rate,
                    "position_seconds": result.position_seconds,
                    "fft_size": result.fft_size,
                    "third_octave": bands.into_iter().map(|(hz, dbfs)| {
                        serde_json::json!({"hz": hz, "dbfs": dbfs})
                    }).collect::<Vec<_>>(),
                }))?;
            } else {
                emit(&result)?;
            }
        }
        Action::Annotate {
            path,
            text,
            seconds,
            output,
            clear,
        } => {
            if clear {
                let stored = match &output {
                    Some(dst) => {
                        let empty: Vec<Annotation> = Vec::new();
                        annotations::save_as(&path, dst, &empty)?;
                        empty
                    }
                    None => {
                        annotations::write(&path, &[])?;
                        Vec::new()
                    }
                };
                emit(
                    &json!({"path": path, "output": output, "annotations": annotations::to_json(&stored)}),
                )?;
                return Ok(());
            }
            let new = if let Some(seconds) = seconds {
                if !seconds.is_finite() || !(0.0..=MAX_SEEK_SECONDS).contains(&seconds) {
                    bail!("seconds must be finite and between 0 and {MAX_SEEK_SECONDS}");
                }
                vec![Annotation {
                    seconds,
                    text: text.unwrap_or_default(),
                }]
            } else {
                annotations::parse_text(
                    &text.context("provide TEXT with [hh:mm:ss:cc] markers, or use --seconds")?,
                )?
            };
            let stored = match &output {
                Some(dst) => {
                    let stored = annotations::read(&path)?;
                    let mut merged = stored;
                    annotations::merge(&mut merged, &new);
                    annotations::save_as(&path, dst, &merged)?;
                    vlog(format!(
                        "saved annotated copy of {} to {} ({} marker(s))",
                        path.display(),
                        dst.display(),
                        merged.len()
                    ));
                    merged
                }
                None => annotations::annotate(&path, &new)?,
            };
            emit(
                &json!({"path": path, "output": output, "annotations": annotations::to_json(&stored)}),
            )?;
        }
        Action::Annotations { path } => {
            let stored = annotations::read(&path)?;
            emit(&json!({"path": path, "annotations": annotations::to_json(&stored)}))?;
        }
        Action::Devices => emit(&viewfrx_audio::devices()?)?,
        Action::Play {
            path,
            device,
            gain,
            gain_db,
            normalize,
            cache,
            rate,
            highpass,
            seconds,
        } => {
            if seconds.is_some_and(|s| !s.is_finite() || s <= 0.0) {
                bail!("seconds must be finite and positive");
            }
            let gain = gain_db.map_or(gain, db_to_gain);
            viewfrx_audio::render::validate_gain(gain)?;
            let command = if normalize {
                // DC-removed peak: DC offset must not steer the auto gain.
                let peak = WaveformCache::new(cache_path(cache))
                    .get(&path, DEFAULT_WAVEFORM_BINS)?
                    .waveform
                    .normalization()?
                    .source_peak;
                Command::PlayNormalized { path, peak }
            } else {
                Command::Play { path }
            };
            let engine = Engine::open(device)?;
            engine.execute(Command::Gain { value: gain })?;
            engine.execute(Command::Rate { value: rate })?;
            engine.execute(Command::Highpass { enabled: highpass })?;
            engine.execute(command)?;
            let start = Instant::now();
            loop {
                thread::sleep(Duration::from_millis(STATUS_POLL_MS));
                let status = engine.status();
                if let Some(error) = status.error {
                    bail!("{error}");
                }
                if status.state == "ended"
                    || seconds.is_some_and(|s| start.elapsed().as_secs_f64() >= s)
                {
                    emit(&status)?;
                    break;
                }
            }
        }
        Action::Session { device } => session(Engine::open(device)?)?,
        Action::Benchmark {
            path,
            sample_rate,
            rate,
            highpass,
        } => {
            let start = Instant::now();
            let mut renderer = viewfrx_audio::render::Renderer::open(&path, sample_rate, rate)?;
            renderer.set_highpass(highpass);
            let opened = start.elapsed();
            let first = renderer.next_frame()?;
            let first_frame_ms = start.elapsed().as_secs_f64() * 1000.0;
            let mut count = 0u64;
            let mut sum_squares = 0f64;
            if let Some(frame) = first {
                count += 1;
                sum_squares += frame.iter().map(|s| (*s as f64).powi(2)).sum::<f64>();
            }
            while let Some(frame) = renderer.next_frame()? {
                count += 1;
                sum_squares += frame.iter().map(|s| (*s as f64).powi(2)).sum::<f64>();
            }
            let elapsed = start.elapsed().as_secs_f64();
            emit(
                &json!({"frames":count,"open_ms":opened.as_secs_f64()*1000.0,"first_frame_ms":first_frame_ms,"elapsed_seconds":elapsed,"realtime_factor":count as f64 / sample_rate as f64 / elapsed,"sum_squares":sum_squares}),
            )?;
        }
    }
    Ok(())
}

fn session(engine: Engine) -> Result<()> {
    emit(&json!({"event":"ready","protocol":1,"version":env!("CARGO_PKG_VERSION")}))?;
    // Bound each request so accidental binary/huge input cannot exhaust memory.
    let mut input = io::stdin().lock();
    loop {
        let mut bytes = Vec::new();
        use std::io::Read;
        let n = input
            .by_ref()
            .take((MAX_REQUEST_BYTES + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        if n == 0 {
            break;
        }
        if bytes.len() > MAX_REQUEST_BYTES {
            bail!("session request exceeds {MAX_REQUEST_BYTES} bytes");
        }
        let result = (|| -> Result<bool> {
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).context("expected a JSON command")?;
            match value.get("op").and_then(|op| op.as_str()) {
                Some("status") if value.as_object().is_some_and(|o| o.len() == 1) => {
                    emit(&engine.status())?;
                }
                Some("quit") if value.as_object().is_some_and(|o| o.len() == 1) => {
                    emit(&json!({"ok":true}))?;
                    return Ok(false);
                }
                Some("annotations") => {
                    let path: PathBuf = serde_json::from_value(
                        value
                            .get("path")
                            .cloned()
                            .context("annotations needs a path")?,
                    )?;
                    let stored = annotations::read(&path)?;
                    vlog(format!(
                        "annotations {}: {} marker(s)",
                        path.display(),
                        stored.len()
                    ));
                    emit(
                        &json!({"ok":true, "path": path, "annotations": annotations::to_json(&stored)}),
                    )?;
                }
                Some("annotate") => {
                    let path: PathBuf = serde_json::from_value(
                        value
                            .get("path")
                            .cloned()
                            .context("annotate needs a path")?,
                    )?;
                    if value
                        .get("clear")
                        .and_then(|c| c.as_bool())
                        .unwrap_or(false)
                    {
                        // Same as `annotate --clear`: remove all markers.
                        annotations::write(&path, &[])?;
                        emit(&json!({"ok":true, "path": path, "annotations": []}))?;
                        return Ok(true);
                    }
                    let new = if let Some(seconds) = value.get("seconds").and_then(|s| s.as_f64()) {
                        if !seconds.is_finite() || !(0.0..=MAX_SEEK_SECONDS).contains(&seconds) {
                            bail!("seconds must be finite and between 0 and {MAX_SEEK_SECONDS}");
                        }
                        vec![Annotation {
                            seconds,
                            text: value
                                .get("text")
                                .and_then(|t| t.as_str())
                                .unwrap_or_default()
                                .to_string(),
                        }]
                    } else {
                        let text = value.get("text").and_then(|t| t.as_str()).context(
                            "annotate needs text with [hh:mm:ss:cc] markers, or seconds",
                        )?;
                        annotations::parse_text(text)?
                    };
                    let stored = annotations::annotate(&path, &new)?;
                    vlog(format!(
                        "annotated {}: {} marker(s)",
                        path.display(),
                        stored.len()
                    ));
                    emit(
                        &json!({"ok":true, "path": path, "annotations": annotations::to_json(&stored)}),
                    )?;
                }
                _ => {
                    vlog(format!("session op: {value}"));
                    engine.execute(serde_json::from_value(value)?)?;
                    emit(&json!({"ok":true,"ack":"accepted"}))?;
                }
            }
            Ok(true)
        })();
        match result {
            Ok(false) => break,
            Ok(true) => {}
            Err(error) => emit(&json!({"ok":false,"error":format!("{error:#}")}))?,
        }
    }
    Ok(())
}
