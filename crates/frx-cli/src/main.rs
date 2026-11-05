use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use frx_audio::{Command, Engine};
use frx_core::{
    catalog::{Catalog, discover_ordered},
    decode::inspect,
    waveform::WaveformCache,
};
use serde::Serialize;
use serde_json::json;
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    name = "frxview",
    version,
    about = "CLI-first audio browser. Structured JSON output; audio stays local."
)]
struct Args {
    #[arg(long, global = true, default_value = ".frxview/library.json")]
    library: PathBuf,
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Register audio files/folders in place; does not copy or decode them.
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
        #[arg(long, default_value_t = 4096)]
        bins: usize,
        #[arg(long, default_value = ".frxview/waveforms")]
        cache: PathBuf,
    },
    /// List current output devices (indices are session-local, not persistent IDs).
    Devices,
    /// Play one file until EOF (or --seconds). Rate changes pitch.
    Play {
        path: PathBuf,
        #[arg(long)]
        device: Option<usize>,
        #[arg(long, default_value_t = 1.0)]
        gain: f32,
        #[arg(long, default_value_t = 1.0)]
        rate: f64,
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
        #[arg(long, default_value_t = 48000)]
        sample_rate: u32,
        #[arg(long, default_value_t = 1.0)]
        rate: f64,
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
    match args.command {
        Action::Import { paths } => {
            let mut catalog = Catalog::load(&args.library)?;
            catalog.import(&paths)?;
            catalog.save(&args.library)?;
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
                Catalog::load(&args.library)?.roots
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
        Action::Waveform { path, bins, cache } => {
            emit(&WaveformCache::new(cache).get(&path, bins)?)?
        }
        Action::Devices => emit(&frx_audio::devices()?)?,
        Action::Play {
            path,
            device,
            gain,
            rate,
            seconds,
        } => {
            if seconds.is_some_and(|s| !s.is_finite() || s <= 0.0) {
                bail!("seconds must be finite and positive");
            }
            let engine = Engine::open(device)?;
            engine.execute(Command::Gain { value: gain })?;
            engine.execute(Command::Rate { value: rate })?;
            engine.execute(Command::Play { path })?;
            let start = Instant::now();
            loop {
                thread::sleep(Duration::from_millis(10));
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
        } => {
            let start = Instant::now();
            let mut renderer = frx_audio::render::Renderer::open(&path, sample_rate, rate)?;
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
    emit(&json!({"event":"ready","protocol":1}))?;
    // Bound each request so accidental binary/huge input cannot exhaust memory.
    let mut input = io::stdin().lock();
    loop {
        let mut bytes = Vec::new();
        use std::io::Read;
        let n = input.by_ref().take(65537).read_until(b'\n', &mut bytes)?;
        if n == 0 {
            break;
        }
        if bytes.len() > 65536 {
            bail!("session request exceeds 64 KiB");
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
                _ => {
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
