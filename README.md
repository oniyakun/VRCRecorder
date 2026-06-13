# VRCRecoder

Tauri v2 Windows desktop app for receiving and recording Spout video streams.

## Prerequisites

- Windows 11
- [Rust](https://rustup.rs/) (edition 2024)
- [Spout2 SDK](https://spout.zeal.co/) — or run `scripts/setup_spout.ps1` to auto-install
- [FFmpeg](https://ffmpeg.org/) — expected at `C:/Program Files/Virtual Desktop Streamer/ffmpeg.exe`

## Dev

```bash
# Install Spout2 SDK
powershell -File scripts/setup_spout.ps1

# Set env vars
$env:SPOUT2_SDK_DIR = "K:/Dev/VRCRecoder/vendor/Spout2/SPOUTSDK"
$env:SPOUT2_LIB_DIR = "K:/Dev/VRCRecoder/vendor/Spout2/BUILD"

# Build
cargo build
```

Frontend is static JS/HTML (`frontend/`), no Node.js or npm required.

## Release Build

```bash
cargo build --release
```

Output at `target/release/VRCRecoder.exe`.

## Stack

- **Backend**: Rust + Tauri v2
- **Frontend**: Vanilla JS + HTML
- **Video**: Spout SDK + FFmpeg
- **Audio**: cpal
- **Integration**: OSC (VRChat), SteamVR Overlay
