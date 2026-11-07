mod app;
mod config;
mod logbuf;
mod model;
mod services;
pub(crate) use logbuf::logln;
#[cfg(feature = "spectrogram")]
mod spectral;
mod theme;
mod widgets;

use clap::Parser;
use eframe::egui;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

static SMOKE_FAILED: AtomicBool = AtomicBool::new(false);

#[derive(Parser, Debug)]
#[command(
    name = "viewfrx-gui",
    version,
    about = "JESDEV audio browser — native desktop frontend"
)]
pub struct Args {
    /// Files/folders forming this session; nothing is restored from previous runs.
    paths: Vec<PathBuf>,
    /// Deprecated compatibility argument; catalogs are no longer read or written.
    #[arg(long, hide = true, default_value = ".")]
    library: PathBuf,
    /// Optional explicit analysis-artifact directory; otherwise temporary.
    #[arg(long, default_value = ":temporary:")]
    cache: PathBuf,
    /// Start with gain zero, useful for safe automated visual checks.
    #[arg(long)]
    muted: bool,
    /// Pipe console log lines to the debug window (in-app console).
    #[arg(long)]
    debug_console: bool,
    /// Start with the spectrogram view enabled (default: off).
    #[cfg(feature = "spectrogram")]
    #[arg(long)]
    spectrogram: bool,
    /// Diagnostic: close after this many seconds (no automatic audible playback).
    #[arg(long)]
    smoke_seconds: Option<f64>,
    /// Diagnostic: save an actual rendered frame before the smoke run exits.
    #[arg(long, requires = "smoke_seconds")]
    screenshot: Option<PathBuf>,
}

fn main() -> eframe::Result {
    let mut args = Args::parse();
    let temporary_cache = tempfile::tempdir().expect("create session cache");
    if args.cache == std::path::Path::new(":temporary:") {
        args.cache = temporary_cache.path().to_path_buf();
    }
    if args.smoke_seconds.is_some_and(|s| {
        !s.is_finite()
            || !(config::runtime::SMOKE_MIN_SECONDS..=config::runtime::SMOKE_MAX_SECONDS)
                .contains(&s)
    }) {
        eprintln!("--smoke-seconds must be between 1 and 120");
        std::process::exit(2);
    }
    let icon = theme::logo();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("viewfrx · JESDEV")
            .with_inner_size(config::layout::WINDOW_SIZE)
            .with_min_inner_size(config::layout::WINDOW_MIN)
            .with_icon(egui::IconData {
                width: icon.width(),
                height: icon.height(),
                rgba: icon.into_raw(),
            }),
        ..Default::default()
    };
    logbuf::push(format!(
        "viewfrx-gui starting, args: {:?}",
        std::env::args().skip(1).collect::<Vec<_>>()
    ));
    let result = eframe::run_native(
        "viewfrx",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(&cc.egui_ctx, args)))),
    );
    if SMOKE_FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
    result
}
