//! WavForge — entry point.
//!
//! Initializes the eframe/egui application window and hands control
//! to [`app::WavForgeApp`]. All application state lives there.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console on Windows in release

mod app;
mod audio;

fn main() -> eframe::Result<()> {
    env_logger::init(); // RUST_LOG=debug for verbose output

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("WavForge")
            .with_inner_size([1024.0, 600.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "WavForge",
        options,
        Box::new(|_cc| Ok(Box::new(app::WavForgeApp::new()))),
    )
}