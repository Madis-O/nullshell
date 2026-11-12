//! NullShell: a standalone terminal emulator for Windows.
//!
//! Module map:
//!   config  - TOML settings
//!   pty     - ConPTY + shell process
//!   term    - terminal emulation, produces a `frame::Frame`
//!   frame   - plain data handed from term to render
//!   render  - wgpu renderer, layers and shaders
//!   input   - key / mouse / paste encoding
//!   app     - window and event loop tying it together

// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod color;
mod config;
mod frame;
mod input;
mod platform;
mod pty;
mod render;
mod term;

use std::io::Write;
use std::path::PathBuf;

fn log_path() -> Option<PathBuf> {
    Some(config::default_path()?.with_file_name("nullshell.log"))
}

fn log(message: &str) {
    eprintln!("{message}");
    if let Some(path) = log_path() {
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{message}");
        }
    }
}

fn main() {
    // A panic in a windowed app would otherwise vanish without a trace.
    std::panic::set_hook(Box::new(|info| log(&format!("panic: {info}"))));

    let mut config_path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" | "-c" => config_path = args.next().map(PathBuf::from),
            "--print-default-config" => {
                print!("{}", config::DEFAULT_CONFIG);
                return;
            }
            _ => {}
        }
    }

    let (cfg, warning) = config::load(config_path);
    if let Some(warning) = &warning {
        log(warning);
    }
    if let Err(e) = app::run(cfg, warning) {
        let message = format!("{e:#}");
        log(&message);
        platform::error_box(&message);
        std::process::exit(1);
    }
}
