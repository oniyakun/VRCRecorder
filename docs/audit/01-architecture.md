# 01 — Architecture & System Overview

## 1. What it is

A Windows desktop app (Tauri v2) that **receives a Spout video stream** (intended: VRChat's
streaming/Spout camera), shows an optional live preview, and **records it to an H.264 MP4**
with **system audio + microphone** mixed in. It integrates with VRChat over OSC (auto-toggle
the in-game Spout camera; follow camera orientation) and exposes a **SteamVR dashboard overlay**
with a controller-bindable "toggle record" action.

- **Backend**: Rust (edition 2024) + Tauri v2.
- **Frontend**: vanilla HTML/CSS/JS in `frontend/`, no bundler. Communicates via Tauri IPC.
- **Video in**: Spout (`spout-rs` + vendored `Spout.dll`) → OpenGL texture.
- **Video out**: FFmpeg CLI (piped raw frames) → H.264 MP4.
- **Audio**: `cpal` (WASAPI). System audio via loopback on the default output device; mic via input device.
- **VR**: raw FFI into `openvr_api.dll` (no `openvr` crate) for a dashboard overlay.

## 2. Process & thread model

Everything runs in one process. Concurrency is built from plain OS threads + `mpsc` channels
+ a few `Arc<Mutex<…>>` / `Arc<AtomicBool>` shared cells. There is **no async runtime**.

| Thread / context | Spawned in | Responsibility | Cadence |
|---|---|---|---|
| **Main / UI** | `main()` → `tauri::Builder::run` | WebView host, IPC command handlers, tray icon, window-close→hide, `poll_events` drain | event-driven |
| **`render-thread`** | `render_thread::spawn` | winit event loop + hidden 1×1 OpenGL 4.6 window; Spout receive, preview render, recording capture, sender scan, elapsed timer | `about_to_wait` woken every **16 ms** (`ControlFlow::WaitUntil`, ~60 Hz) |
| **`ffmpeg-writer`** | `FfmpegEncoder::start` (per recording) | Drains the bounded frame channel → writes raw frames to ffmpeg stdin; waits for ffmpeg exit | blocks on channel |
| ffmpeg stderr reader | `FfmpegEncoder::start` | Logs ffmpeg stderr lines | per line; **detached via `mem::forget`** |
| **`encoder-finish`** | `RecordingPipeline::stop` (per stop) | Closes ffmpeg, waits, then runs the audio-merge ffmpeg pass, renames final file | one-shot |
| **`mjpeg-server`** + `mjpeg-client` | `mjpeg_server::start` | Serves the preview as MJPEG-over-HTTP on `127.0.0.1:<random>`; one thread per client | per client, source rate |
| OSC receiver | `OscReceiver::start` | UDP listener on `127.0.0.1:<osc_port>`, 500 ms read timeout | event-driven |
| **`steamvr-overlay`** | `steamvr::spawn_overlay` | OpenVR init, overlay create, event/input poll, texture swap on record-state change | sleeps **11 ms**/iter |
| cpal audio (×1–2) | `AudioCapture::start`, `MicCapture::start` | WASAPI callbacks append s16le PCM to shared buffers | driver callback |
| ad-hoc one-shot | `select_output_dir`, `send_spout_streaming_osc` | folder dialog (`rfd`); fire-and-forget OSC sends | one-shot |

### Command / event channels (`src/channels.rs`)

- **UI → render thread**: `RenderCommand` over `mpsc` (`ConnectSender`, `DisconnectSender`,
  `StartRecording`, `StopRecording`, `UpdateConfig{…}`, `UpdateOrientation{…}`, `Shutdown`).
- **Render thread → UI**: `RenderEvent` over `mpsc`, drained by the frontend polling
  `poll_events` every **300 ms** (`SenderListUpdated`, `SenderConnected`, `SenderDisconnected`,
  `RecordingStarted`, `RecordingStopped{final_path}`, `RecordingElapsed`, `ConfigChanged`,
  `Error`, `ResolutionChanged`).
- The SteamVR overlay thread sends `Start/StopRecording` directly into the same command channel.

> Note: `RenderEvent` is serialized for the frontend but delivered by **polling**, not Tauri's
> native event emitter.

## 3. Video data flow

```
VRChat / Spout sender
   │  (GPU shared texture, IPC)
   ▼
SpoutReceiver.receive_texture()           src/spout/receiver.rs
   │  → copies into our GL texture (RGBA8) src/render/texture.rs
   ▼
GL texture (spout_tex)  ──────────────┬───────────────────────────────┐
   │                                  │                                │
   │ PREVIEW (preview_enabled only)   │ RECORDING (recording only)     │
   ▼                                  ▼                                │
PreviewRenderer.render()         PboReader.begin_frame()/end_frame()   │
 (UV transform: mirror/portrait/  (double-buffered async PBO readback) │
  vflip via GPU)                       │                               │
   │ FBO → glReadPixels (sync)         │ RGBA Vec<u8>                   │
   ▼                                   ▼                               │
RGBA → strip α → JPEG q80         bounded channel (cap 60 frames) ──────┘
   │                                   │
   ▼                                   ▼
preview_buf (Arc<Mutex<Vec<u8>>>) ffmpeg-writer → ffmpeg stdin (rawvideo rgba, -r 60)
   │                                   │
   ▼                                   ▼  -vf transpose=2?/hflip?/vflip?,format=nv12|yuv420p
MJPEG server → <img> in WebView   H.264 (NVENC/AMF/QSV/x264/openh264) → <out>_video.mp4
```

Key transforms live in **two different places** and must agree:
- **Preview** orientation = GPU UV manipulation in `src/render/preview.rs` (`build_quad_vertices`).
- **Recording** orientation = FFmpeg `-vf` filter chain in `src/record/encoder.rs`
  (`transpose=2` for portrait, `hflip` for mirror, `vflip`).

### Frame timing model (important for the audit)

- The render loop wakes ~every 16 ms and only does capture/preview work **when Spout reported a
  new frame** (`update_spout()` return value).
- The encoder is told the input is **constant 60 fps** (`fps = 60.0`, `-r 60`).
- Recording duration used for A/V alignment = **`frame_count / 60.0`** (`pipeline.rs`).
- Frames are **dropped** (not blocked) if the encoder channel is full during steady-state
  capture; the final flush uses a **blocking** send so the last frames are never dropped.

→ See `04-observations.md` R1/R2 for the correctness implications of the fixed-60 assumption.

## 4. Audio data flow

- **System audio** (`record/audio.rs`): `cpal` builds an *input* stream on the **default output
  device** (WASAPI loopback). Sample rate & channel count come from the device's default output
  config. f32 samples → clamped → s16le interleaved → appended to an in-RAM `Vec<u8>`.
- **Microphone** (`record/mic.rs`): `cpal` input stream on the selected (or default) input
  device; same f32 → s16le path into a separate in-RAM buffer.
- Both buffers **accumulate for the entire recording** and are drained only at `stop()`.
- **Merge** (`pipeline.rs::merge_audio_video`): PCM written to temp `.sys.pcm` / `.mic.pcm`,
  fed to ffmpeg as `s16le` inputs.
  - sys + mic → `amix=inputs=2:duration=first:weights=1 0.5` (system full, mic at 0.5).
  - one source → mapped directly.
  - Output: `-c:v copy -c:a aac -b:a 192k`, trimmed to `-t frame_count/60`.

## 5. External dependencies & integration points

| Dependency | How it's located / loaded | Notes |
|---|---|---|
| **Spout.dll** | Embedded at build (`include_bytes!`), extracted to cache dir at startup (`bundle.rs`), **delay-loaded** (`build.rs` `/DELAYLOAD`), dir added via `SetDllDirectoryW` | Built from vendored `vendor/Spout2` via `scripts/setup_spout.ps1` (CMake) |
| **FFmpeg** | `ffmpeg.exe` + 7 av*/sw* DLLs embedded at build from the **Virtual Desktop Streamer** path, extracted to cache dir; runtime fallback to that VDS path | **Build fails to compile if VDS ffmpeg is absent** (`include_bytes!(env!(...))`). DLL versions are pinned (avcodec-61, etc.) |
| **VRChat OSC** | UDP to/from `127.0.0.1` | **Receive** `/usercamera/OrientationIsLandscape` on `osc_port` (default 9001). **Send** `/usercamera/Streaming <bool>` to `vrchat_port` (default 9000) on record start/stop and preview toggle |
| **OpenVR / SteamVR** | `openvr_api.dll` located via registry (`HKCU\Software\Valve\Steam` → SteamVR install), loaded with `LoadLibraryW`, **raw fn-table FFI** | Interface versions hard-coded: `IVROverlay_027`, `IVRInput_010`, `IVRApplications_007`. Auto-launch via `vrstartup.exe` |

## 6. Persistent state & file layout (runtime)

- **Config**: `%APPDATA%/vrc-recorder/config.json` (`dirs::config_dir`), pretty JSON, saved on
  every change. Schema in `src/config.rs` (`AppConfig`).
- **Logs**: `%LOCALAPPDATA%/vrc-recorder/vrc-recorder.log` (`dirs::cache_dir`); panics appended.
- **Cache/runtime extraction dir** (`dirs::cache_dir`/vrc-recorder): `ffmpeg.exe`, av/sw DLLs,
  `Spout.dll`, SteamVR manifests (`manifest.vrmanifest`, `action_manifest.json`, controller
  bindings), `steamvr_overlay.png`.
- **Recordings**: `output_dir` (default `dirs::video_dir()`, i.e. *Videos*).
  - Intermediate: `VRCRecorder_<start-ts>_video.mp4`, `…sys.pcm`, `…mic.pcm`.
  - Final: `VRCRecorder_<stop-ts>.mp4` (intermediate deleted on success).

## 7. Lifecycle

- **Startup** (`main()`): init logging + panic hook → `bundle::extract()` → load config →
  spawn render thread + MJPEG server → build Tauri app → on setup: tray icon, `sync_osc`,
  spawn SteamVR overlay.
- **Window close**: intercepted (`prevent_close`) → window hidden to tray. Quit only via the
  tray menu's **Quit** (or `VREvent_Quit` from SteamVR).
- **Resolution change** while connected: render thread stops any recording, emits
  `ResolutionChanged`; the UI thread then **relaunches the whole process** (`spawn` new exe +
  `process::exit(0)` inside `poll_events`).
- **Shutdown**: `RecordingPipeline::drop` / `FfmpegEncoder::drop` close the pipe and join the
  writer thread; GL resources freed on the render thread.

## 8. Build & packaging

- `build.rs`: generates `icons/icon.ico` from `logo.png`; runs `tauri_build::build()`; copies
  `Spout.dll`, `logo.png`, sound files; sets `rustc-env` paths for all embedded binaries;
  configures `/DELAYLOAD:Spout.dll`.
- Requires env vars `SPOUT2_SDK_DIR` / `SPOUT2_LIB_DIR` (defaults in `.cargo/config.toml` point
  into `vendor/Spout2`).
- Release profile: `opt-level=3`, `lto="thin"`, `codegen-units=4`.
- Tauri capabilities (`capabilities/default.json`) grant only `core:default`,
  `core:window:default`, `core:event:default`. **CSP is `null`** (disabled) in `tauri.conf.json`.
