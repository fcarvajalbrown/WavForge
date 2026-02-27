//! WavForge application state and main UI loop.
//!
//! [`WavForgeApp`] is the top-level struct handed to eframe. It owns all
//! editor state and drives the egui update loop on every frame.

use crate::audio::engine::{AudioEngine, EngineCommand};
use egui::Context;
use std::path::PathBuf;

/// Top-level application state.
pub struct WavForgeApp {
    /// Decoded PCM samples for the currently open file.
    /// `None` if no file is loaded.
    samples: Option<Vec<f32>>,

    /// Sample rate of the loaded file (e.g. 44100, 48000).
    sample_rate: u32,

    /// Number of channels in the loaded file (1 = mono, 2 = stereo).
    channels: usize,

    /// Path of the currently open file, for display in the title bar.
    file_path: Option<PathBuf>,

    /// Audio playback engine. Owns the cpal stream and playback thread.
    engine: AudioEngine,

    /// Whether audio is currently playing.
    is_playing: bool,

    /// Error message to display in the UI, if any.
    error: Option<String>,
}

impl WavForgeApp {
    /// Creates a new, empty application — no file loaded.
    pub fn new() -> Self {
        Self {
            samples: None,
            sample_rate: 44100,
            channels: 1,
            file_path: None,
            engine: AudioEngine::new(),
            is_playing: false,
            error: None,
        }
    }

    /// Opens a file via a native dialog and decodes it into PCM samples.
    /// Replaces any currently loaded file.
    fn open_file(&mut self) {
        let path = rfd::FileDialog::new()
            .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "aiff", "aif"])
            .pick_file();

        let Some(path) = path else { return };

        match crate::audio::decoder::decode_file(&path) {
            Ok((samples, sample_rate, channels)) => {
                self.samples = Some(samples);
                self.sample_rate = sample_rate;
                self.channels = channels;
                self.file_path = Some(path);
                self.is_playing = false;
                self.error = None;
                // Stop any existing playback
                let _ = self.engine.send(EngineCommand::Stop);
            }
            Err(e) => {
                self.error = Some(format!("Failed to open file: {e}"));
            }
        }
    }

    /// Sends a Play command to the audio engine with the current samples.
    fn play(&mut self) {
        let Some(samples) = &self.samples else { return };
        let cmd = EngineCommand::Play {
            samples: samples.clone(),
            sample_rate: self.sample_rate,
            channels: self.channels,
        };
        if let Err(e) = self.engine.send(cmd) {
            self.error = Some(format!("Playback error: {e}"));
        } else {
            self.is_playing = true;
        }
    }

    /// Sends a Pause command to the audio engine.
    fn pause(&mut self) {
        let _ = self.engine.send(EngineCommand::Pause);
        self.is_playing = false;
    }

    /// Sends a Stop command to the audio engine and resets playback state.
    fn stop(&mut self) {
        let _ = self.engine.send(EngineCommand::Stop);
        self.is_playing = false;
    }

    /// Renders the top menu bar.
    fn ui_menu(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open…  Ctrl+O").clicked() {
                    ui.close_menu();
                    self.open_file();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }

    /// Renders the transport toolbar (play / pause / stop).
    fn ui_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let has_file = self.samples.is_some();

            if ui
                .add_enabled(has_file && !self.is_playing, egui::Button::new("▶ Play"))
                .clicked()
            {
                self.play();
            }

            if ui
                .add_enabled(has_file && self.is_playing, egui::Button::new("⏸ Pause"))
                .clicked()
            {
                self.pause();
            }

            if ui
                .add_enabled(has_file, egui::Button::new("⏹ Stop"))
                .clicked()
            {
                self.stop();
            }

            ui.separator();

            // File name display
            if let Some(path) = &self.file_path {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                ui.label(name);
            } else {
                ui.label(egui::RichText::new("No file loaded").weak());
            }
        });
    }

    /// Renders the waveform area — placeholder for M0, full renderer in M1.
    fn ui_waveform(&self, ui: &mut egui::Ui) {
        let available = ui.available_rect_before_wrap();

        if self.samples.is_some() {
            // M0 placeholder: just show a grey box with a label
            ui.painter().rect_filled(
                available,
                4.0,
                egui::Color32::from_gray(30),
            );
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("Waveform renderer coming in M1")
                        .color(egui::Color32::from_gray(120)),
                );
            });
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("Open a file to get started  (File → Open)")
                        .weak()
                        .size(16.0),
                );
            });
        }
    }

    /// Renders the status bar at the bottom of the window.
    fn ui_statusbar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(samples) = &self.samples {
                let duration_secs =
                    samples.len() as f64 / (self.sample_rate as f64 * self.channels as f64);
                ui.label(format!(
                    "{} Hz  ·  {} ch  ·  {:.2}s  ·  {} samples",
                    self.sample_rate,
                    self.channels,
                    duration_secs,
                    samples.len() / self.channels,
                ));
            }

            // Error display
            if let Some(err) = &self.error {
                ui.separator();
                ui.label(egui::RichText::new(err).color(egui::Color32::RED));
            }
        });
    }
}

impl eframe::App for WavForgeApp {
    /// Called every frame by eframe. Builds the full UI.
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Handle Ctrl+O globally
        if ctx.input(|i| i.key_pressed(egui::Key::O) && i.modifiers.ctrl) {
            self.open_file();
        }

        // Menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            self.ui_menu(ui);
        });

        // Transport toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(4.0);
            self.ui_toolbar(ui);
            ui.add_space(4.0);
        });

        // Status bar
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.add_space(2.0);
            self.ui_statusbar(ui);
            ui.add_space(2.0);
        });

        // Central waveform panel
        egui::CentralPanel::default().show(ctx, |ui| {
            self.ui_waveform(ui);
        });
    }
}