//! Audio file decoding via Symphonia.
//!
//! Provides a single public function [`decode_file`] that reads any supported
//! audio format and returns interleaved f32 PCM samples normalized to [-1.0, 1.0],
//! along with the sample rate and channel count.
//!
//! Supported formats: WAV, MP3, FLAC, OGG/Vorbis, AIFF.

use std::path::Path;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decodes an audio file into interleaved f32 PCM samples.
///
/// # Arguments
/// * `path` — Path to the audio file. Extension is used as a format hint.
///
/// # Returns
/// A tuple of `(samples, sample_rate, channels)` where:
/// - `samples` — Interleaved f32 PCM, normalized to [-1.0, 1.0]
/// - `sample_rate` — e.g. 44100, 48000
/// - `channels` — 1 for mono, 2 for stereo
///
/// # Errors
/// Returns a string error message if the file cannot be opened, probed,
/// or decoded.
pub fn decode_file(path: &Path) -> Result<(Vec<f32>, u32, usize), String> {
    // Open the file and wrap it in a MediaSourceStream
    let file = std::fs::File::open(path)
        .map_err(|e| format!("Cannot open file: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // Give symphonia a format hint based on file extension
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    // Probe the media source to detect format + locate the default track
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Unsupported format: {e}"))?;

    let mut format = probed.format;

    // Find the first audio track
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or("No audio track found")?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let sample_rate = codec_params
        .sample_rate
        .ok_or("Unknown sample rate")?;

    let channels = codec_params
        .channels
        .map(|c| c.count())
        .ok_or("Unknown channel count")?;

    // Build the decoder for this track
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Failed to create decoder: {e}"))?;

    let mut samples: Vec<f32> = Vec::new();

    // Decode packet by packet until EOF
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break; // clean EOF
            }
            Err(SymphoniaError::ResetRequired) => {
                // Some formats signal a reset (e.g. chained OGG streams).
                // For v1 we treat this as end of first stream.
                break;
            }
            Err(e) => return Err(format!("Packet error: {e}")),
        };

        // Skip packets from other tracks (e.g. cover art)
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder
            .decode(&packet)
            .map_err(|e| format!("Decode error: {e}"))?;

        // Convert whatever sample format symphonia gives us into f32
        convert_buffer(&decoded, &mut samples);
    }

    Ok((samples, sample_rate, channels))
}

/// Converts a [`AudioBufferRef`] of any sample type into f32 samples,
/// appending them to `out`. All formats are normalized to [-1.0, 1.0].
fn convert_buffer(buffer: &AudioBufferRef<'_>, out: &mut Vec<f32>) {
    match buffer {
        AudioBufferRef::F32(buf) => {
            for plane in buf.planes().planes() {
                out.extend_from_slice(plane);
            }
        }
        AudioBufferRef::F64(buf) => {
            for plane in buf.planes().planes() {
                out.extend(plane.iter().map(|&s| s as f32));
            }
        }
        AudioBufferRef::S16(buf) => {
            for plane in buf.planes().planes() {
                out.extend(plane.iter().map(|&s| s as f32 / i16::MAX as f32));
            }
        }
        AudioBufferRef::S24(buf) => {
            for plane in buf.planes().planes() {
                // Symphonia's S24 is stored in i32
                out.extend(plane.iter().map(|&s| s.0 as f32 / 8_388_607.0));
            }
        }
        AudioBufferRef::S32(buf) => {
            for plane in buf.planes().planes() {
                out.extend(plane.iter().map(|&s| s as f32 / i32::MAX as f32));
            }
        }
        AudioBufferRef::U8(buf) => {
            for plane in buf.planes().planes() {
                out.extend(plane.iter().map(|&s| (s as f32 - 128.0) / 128.0));
            }
        }
        // Catch-all for any future formats symphonia adds
        _ => {
            log::warn!("Unsupported sample format — skipping buffer");
        }
    }
}