# WavForge

A lightweight, clean audio editor. No telemetry, no plugin bloat, no UI clutter.

> Think Audacity circa 2015 — built in Rust.

## Features (v1)

- Open WAV, MP3, FLAC, OGG, AIFF
- Waveform display with zoom and scroll
- Selection, cut, copy, paste, trim, silence
- Undo/redo (200 steps)
- Export to WAV

## Build

```bash
cargo build --release
```

Requires Rust stable. No system dependencies beyond a working audio backend (ALSA on Linux, CoreAudio on macOS, WASAPI on Windows).

## Status

🚧 Early development — M0 skeleton in progress.

## License

GPL-3.0
