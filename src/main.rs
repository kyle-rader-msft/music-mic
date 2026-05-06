#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod ui;

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use tracing_subscriber::{EnvFilter, fmt};

// Sized so the whole UI (header + 3 cards + Start button + Teams hint) fits
// at launch with a small breathing margin. The min size keeps the layout
// from collapsing if the user shrinks the window.
const DEFAULT_W: f64 = 480.0;
const DEFAULT_H: f64 = 760.0;
const MIN_W: f64 = 380.0;
const MIN_H: f64 = 540.0;

fn main() {
    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_target(false)
        .init();

    let window = WindowBuilder::new()
        .with_title("music-mic")
        .with_inner_size(LogicalSize::new(DEFAULT_W, DEFAULT_H))
        .with_min_inner_size(LogicalSize::new(MIN_W, MIN_H))
        .with_resizable(true);

    dioxus::LaunchBuilder::new()
        .with_cfg(Config::new().with_window(window))
        .launch(ui::App);
}
