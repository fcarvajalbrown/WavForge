//! Audio playback engine powered by `cpal`.
//!
//! [`AudioEngine`] owns a `cpal` output stream and a background playback
//! state machine driven by [`EngineCommand`]s sent from the UI thread.
//!
//! # Threading model
//!
//! ```text
//! UI thread                    cpal audio callback (real-time thread)
//! ─────────────────────────    ──────────────────────────────────────
//! engine.send(Play { .. })  →  crossbeam_channel::Receiver<EngineCommand>
//!                              reads commands, updates PlaybackState,
//!                              writes samples to output buffer
//! engine.send(Pause)        →  sets state to Paused, cursor holds position
//! engine.send(Stop)         →  resets cursor to 0, clears samples
//! ```
//!
//! The audio callback is intentionally allocation-free. All data needed for
//! playback (`Arc<[f32]>`) is sent over the channel before the stream starts.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, OutputCallbackInfo, SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use std::sync::Arc;

/// Commands the UI thread sends to the audio engine.
pub enum EngineCommand {
    /// Begin playback of the provided samples from the current cursor position.
    Play {
        /// Interleaved f32 PCM samples, normalized to [-1.0, 1.0].
        samples: Vec<f32>,
        /// Sample rate of the audio data (e.g. 44100).
        sample_rate: u32,
        /// Number of interleaved channels (1 = mono, 2 = stereo).
        channels: usize,
    },
    /// Pause playback, holding the current cursor position.
    Pause,
    /// Stop playback and reset the cursor to the beginning.
    Stop,
}

/// Internal state of the playback cursor, shared inside the audio callback.
struct PlaybackState {
    /// The samples currently loaded for playback.
    samples: Arc<[f32]>,
    /// Current read position within `samples` (in interleaved sample indices).
    cursor: usize,
    /// Whether playback is active.
    playing: bool,
}

impl PlaybackState {
    /// Creates a stopped, empty playback state.
    fn empty() -> Self {
        Self {
            samples: Arc::from(vec![].into_boxed_slice()),
            cursor: 0,
            playing: false,
        }
    }
}

/// The public-facing audio engine handle.
///
/// Dropping this struct closes the cpal stream and stops playback.
pub struct AudioEngine {
    /// Sender half of the command channel. Kept alive to avoid channel closure.
    tx: Sender<EngineCommand>,
    /// The active cpal output stream. Must be kept alive for audio to play.
    _stream: Option<Stream>,
}

impl AudioEngine {
    /// Creates a new audio engine, opening the default output device.
    ///
    /// Falls back gracefully if no output device is available — the engine
    /// will still accept commands but produce no audio.
    pub fn new() -> Self {
        match Self::try_build() {
            Ok(engine) => engine,
            Err(e) => {
                log::error!("Failed to initialize audio engine: {e}. Audio disabled.");
                // Return a dummy engine with a disconnected sender
                let (tx, _rx) = crossbeam_channel::unbounded();
                Self { tx, _stream: None }
            }
        }
    }

    /// Sends a command to the audio engine.
    ///
    /// # Errors
    /// Returns an error string if the audio thread has disconnected (which
    /// should not happen during normal operation).
    pub fn send(&self, cmd: EngineCommand) -> Result<(), String> {
        self.tx
            .send(cmd)
            .map_err(|_| "Audio engine disconnected".to_string())
    }

    /// Attempts to build a working cpal stream and command channel.
    fn try_build() -> Result<Self, String> {
        let host = cpal::default_host();

        let device: Device = host
            .default_output_device()
            .ok_or("No output device found")?;

        let supported_config = device
            .default_output_config()
            .map_err(|e| format!("No default output config: {e}"))?;

        log::info!(
            "Audio device: {} | format: {:?} | rate: {}",
            device.name().unwrap_or_default(),
            supported_config.sample_format(),
            supported_config.sample_rate().0,
        );

        let config: StreamConfig = supported_config.clone().into();
        let (tx, rx) = crossbeam_channel::unbounded::<EngineCommand>();

        let stream = match supported_config.sample_format() {
            SampleFormat::F32 => build_stream::<f32>(&device, &config, rx)?,
            SampleFormat::I16 => build_stream::<i16>(&device, &config, rx)?,
            SampleFormat::U16 => build_stream::<u16>(&device, &config, rx)?,
            fmt => return Err(format!("Unsupported output sample format: {fmt:?}")),
        };

        stream.play().map_err(|e| format!("Stream play error: {e}"))?;

        Ok(Self {
            tx,
            _stream: Some(stream),
        })
    }
}

/// Builds a typed cpal output stream for a given sample format `S`.
///
/// The audio callback reads [`EngineCommand`]s from `rx` on every call,
/// then fills the output buffer from the current [`PlaybackState`].
/// This function is generic to satisfy cpal's typed sample API.
fn build_stream<S>(
    device: &Device,
    config: &StreamConfig,
    rx: Receiver<EngineCommand>,
) -> Result<Stream, String>
where
    S: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
{
    let mut state = PlaybackState::empty();
    let channels = config.channels as usize;

    let stream = device
        .build_output_stream(
            config,
            // Audio callback — runs on the real-time thread.
            // RULES: no allocation, no locking, no blocking.
            move |output: &mut [S], _info: &OutputCallbackInfo| {
                // Drain all pending commands non-blockingly
                loop {
                    match rx.try_recv() {
                        Ok(EngineCommand::Play { samples, .. }) => {
                            state.samples = Arc::from(samples.into_boxed_slice());
                            state.cursor = 0;
                            state.playing = true;
                        }
                        Ok(EngineCommand::Pause) => {
                            state.playing = false;
                        }
                        Ok(EngineCommand::Stop) => {
                            state.playing = false;
                            state.cursor = 0;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => break,
                    }
                }

                // Fill output buffer
                for frame in output.chunks_mut(channels) {
                    if state.playing && state.cursor + channels <= state.samples.len() {
                        for (out_sample, &src) in
                            frame.iter_mut().zip(&state.samples[state.cursor..])
                        {
                            *out_sample = S::from_sample(src);
                        }
                        state.cursor += channels;
                    } else {
                        // Silence — either paused, stopped, or end of file
                        for s in frame.iter_mut() {
                            *s = S::from_sample(0.0f32);
                        }
                        if state.cursor >= state.samples.len() {
                            state.playing = false;
                        }
                    }
                }
            },
            // Error callback
            |e| log::error!("Audio stream error: {e}"),
            None,
        )
        .map_err(|e| format!("Failed to build output stream: {e}"))?;

    Ok(stream)
}