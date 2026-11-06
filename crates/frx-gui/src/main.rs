mod app;
mod config;
mod model;
mod services;
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
    name = "frxview-gui",
    version,
    about = "JESDEV audio browser — native desktop frontend"
)]
pub struct Args {
    /// Files/folders to register in the local catalog on startup.
    paths: Vec<PathBuf>,
    #[arg(long, default_value = ".frxview/library.json")]
    library: PathBuf,
    #[arg(long, default_value = ".frxview/waveforms")]
    cache: PathBuf,
    /// Start with gain zero, useful for safe automated visual checks.
    #[arg(long)]
    muted: bool,
    /// Diagnostic: close after this many seconds (no automatic audible playback).
    #[arg(long)]
    smoke_seconds: Option<f64>,
    /// Diagnostic: save an actual rendered frame before the smoke run exits.
    #[arg(long, requires = "smoke_seconds")]
    screenshot: Option<PathBuf>,
}

fn main() -> eframe::Result {
    let args = Args::parse();
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
            .with_title("frxview · JESDEV")
            .with_inner_size(config::layout::WINDOW_SIZE)
            .with_min_inner_size(config::layout::WINDOW_MIN)
            .with_icon(egui::IconData {
                width: icon.width(),
                height: icon.height(),
                rgba: icon.into_raw(),
            }),
        ..Default::default()
    };
    let result = eframe::run_native(
        "frxview",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(&cc.egui_ctx, args)))),
    );
    if SMOKE_FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
    result
}
