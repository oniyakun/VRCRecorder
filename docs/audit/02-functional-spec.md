# 02 — Functional Spec & Behavior Inventory

The **intended** behavior of every user-facing feature, config field, IPC surface, and
integration. This is the contract the audit checks observed behavior against. Where the code's
intent is ambiguous or self-contradictory, it is flagged `⚠` and cross-referenced to
`04-observations.md`.

---

## 1. Source / Spout connection

| Behavior | Intended | Source |
|---|---|---|
| Sender discovery | Every 2 s the render thread lists active Spout senders and pushes `SenderListUpdated`; UI shows a clickable list under **Source** | `render_thread.rs::scan_senders`, `spout/receiver.rs::list_senders` |
| Auto-connect | If not connected and senders exist, auto-connect to saved `last_sender` (if still present) else the first sender | `scan_senders` |
| Manual connect | Clicking a sender → `connect_sender` command; on success emits `SenderConnected{name,w,h}` and UI shows resolution | `main.rs`, `render_thread.rs::connect_sender` |
| Manual disconnect | `disconnect_sender` clears the stream and emits `SenderDisconnected` | `render_thread.rs::disconnect_sender` ⚠ auto-connect re-connects within 2 s — see C4 |
| Reconnect on drop | When a connected sender disappears, retry on a backoff (60→120→300 tick intervals) | `spout/receiver.rs::update` |
| Resolution change | If the connected sender's resolution changes, stop recording, emit `ResolutionChanged`, **relaunch the app** | `render_thread.rs::update_spout`, `main.rs::poll_events` ⚠ see R4 |

## 2. Preview

| Behavior | Intended | Source |
|---|---|---|
| Toggle | **Preview** toggle persists `preview_enabled` and starts/stops the MJPEG `<img>` stream | `frontend/main.js`, `render_thread.rs` |
| Rendering | Per new Spout frame, render with current mirror/portrait/vflip, downscale large sources, JPEG-encode (q80) into the shared buffer | `render_thread.rs::update_preview` |
| Transport | MJPEG-over-HTTP `multipart/x-mixed-replace` on `127.0.0.1:<port>`; `<img src>` consumes it natively | `mjpeg_server.rs`, `main.js::startPreview` |
| Auto-disable on record | Starting a recording forces `preview_enabled=false` and frees preview GPU resources, to save resources | `render_thread.rs::start_recording` |
| Resolution cap | Preview downscaled so the largest dimension ≤ 1920 | `update_preview` ⚠ comment says "1080p"; logic caps at 1920 |

## 3. Recording

| Behavior | Intended | Source |
|---|---|---|
| Start | `start_recording`: auto-disable preview, send `/usercamera/Streaming true` (if `auto_camera`), start ffmpeg encoder + PBO reader + system audio + (optional) mic; emit `RecordingStarted`; play `start.mp3` | `render_thread.rs::start_recording`, `record/pipeline.rs::start` |
| Capture | Each new frame: PBO async readback → push to encoder; dropped if encoder channel full; periodic mic-device check every 2 s | `pipeline.rs::process_frame` |
| Elapsed | `RecordingElapsed{secs}` emitted every 1 s; UI shows `REC HH:MM:SS` | `render_thread.rs::emit_elapsed` |
| Stop | `stop_recording`: stop+drain audio/mic (50 ms settle), flush remaining PBO frames + one synchronous capture of the latest texture, then on a background thread finish ffmpeg and merge audio; send `/usercamera/Streaming false` (if `auto_stop_camera`); emit `RecordingStopped`; play `stop.mp3` | `render_thread.rs::stop_recording`, `pipeline.rs::stop` |
| Output naming | Intermediate `VRCRecorder_<start-ts>_video.mp4`; final `VRCRecorder_<stop-ts>.mp4`; intermediate deleted on success | `pipeline.rs` ⚠ start vs stop timestamp differ; collision risk U7 |
| Audio-merge fallback | If the merge ffmpeg pass fails, keep the **video-only** file as the final output | `pipeline.rs::stop` |
| Final path feedback | `RecordingStopped.final_path` is intended to tell the UI where the file landed | `channels.rs` ⚠ always `None` today — U1 |

### Encoder selection (auto, cached once)
Priority **h264_nvenc → h264_amf → h264_qsv → libx264 → libopenh264**, each with low-latency
flags to avoid multi-second internal buffering. Pixel format `nv12` (NVENC/QSV) or `yuv420p`
(others). Input is `rawvideo rgba` at `-r 60`; bitrate `-b:v <video_bitrate>`. `src/record/encoder.rs`.

### Frame-rate / duration contract ⚠
Output is assembled assuming **constant 60 fps**, and audio is aligned to `frame_count/60`.
The audit must establish the **actual** Spout delivery rate and how drift/drops affect duration
and A/V sync. See R1/R2.

## 4. Video transform options

| Option | Intended effect | Preview impl | Record impl |
|---|---|---|---|
| **Mirror (hflip)** | Horizontal flip | UV `u→1-u` | `hflip` |
| **Portrait Mode** | 90° rotation; output W/H swapped | UV `(1-v, u)` + swap dims | `transpose=2` |
| **Vertical Flip** | Vertical flip | UV `v→1-v` | `vflip` |

Filter order (record): `transpose=2`(portrait) → `hflip`(mirror) → `vflip` → `format`.
⚠ Preview and record use independent code paths and must produce identical orientation for all
8 combinations — verify (R8). Mirror/portrait are also mirrored onto the bottom **dock** buttons.

## 5. Audio

| Behavior | Intended | Source |
|---|---|---|
| System audio | Always attempted via WASAPI loopback on default output device; if it fails, record video-only (warn) | `record/audio.rs`, `pipeline.rs::start` |
| Microphone | Captured when `mic_enabled`; device = `mic_device` or system default | `record/mic.rs` |
| Mic device list | `list_mic_devices` populates the dropdown; "Default Device" = empty value | `main.rs`, `main.js::loadMicDevices` |
| Mic hot-swap | During recording, every 2 s, if the **default input device changed**, restart mic capture | `pipeline.rs::check_mic_device` ⚠ resets to *default*, breaking an explicit selection — C3 |
| Mix | System at weight 1.0, mic at 0.5; AAC 192 kbps | `pipeline.rs::merge_audio_video` ⚠ fixed levels — U5 |
| Format | s16le PCM, device-native sample rate & channel count | `audio.rs`, `mic.rs` ⚠ mismatched rates/surround — R6 |

## 6. VRChat OSC integration

| Behavior | Intended | Source |
|---|---|---|
| Enable | `osc_enabled` starts a UDP listener on `osc_port` (default 9001) | `main.rs::sync_osc`, `osc.rs` |
| Orientation follow | On `/usercamera/OrientationIsLandscape`, set portrait = `!landscape`, persist, emit `ConfigChanged` | `main.rs::sync_osc`, `render_thread.rs` |
| Auto Spout on record | If `auto_camera`, send `/usercamera/Streaming true` to `vrchat_port` (default 9000) at record start (also on preview enable) | `render_thread.rs::send_camera_osc` |
| Auto Stop Spout | If `auto_stop_camera`, send `/usercamera/Streaming false` at record stop (also on preview disable) | `render_thread.rs::send_camera_stop_osc` |
| Ports configurable | `osc_port` (listen) and `vrchat_port` (send) editable in UI | `index.html` ⚠ send-port field is mis-wired — C2 |

## 7. SteamVR overlay

| Behavior | Intended | Source |
|---|---|---|
| Enable | `steamvr_overlay` (default on) spawns the overlay thread; otherwise no-op | `steamvr/mod.rs::spawn_overlay` |
| Init | Wait for `openvr_api.dll`, init as Overlay app, register app + action manifests, create dashboard overlay, set panel texture | `steamvr/mod.rs::overlay_main` |
| Auto-launch | `steamvr_auto_launch` (default off): if SteamVR isn't up, launch it once via `vrstartup.exe`; flag read live so UI toggles apply while waiting | `steamvr/mod.rs`, `ffi.rs::launch_steamvr` *(in-flight feature)* |
| Toggle record action | `/actions/vrc_recorder/in/toggle_record` digital action toggles recording on press | `steamvr/mod.rs` main loop |
| Status panel | 512×256 PNG; green "READY" / red "RECORDING", regenerated on state change | `steamvr/texture.rs` |
| Shutdown | On `VREvent_Quit`, shut down OpenVR and exit the thread | `steamvr/mod.rs` |

## 8. Persistence, tray, lifecycle

| Behavior | Intended | Source |
|---|---|---|
| Config load/save | Load `config.json` at start (defaults if missing/corrupt); save on every change; unknown/missing fields default via `#[serde(default)]` | `config.rs` |
| Output folder | Picker via `select_output_dir` (native `rfd` dialog on a worker thread); persisted | `main.rs`, `main.js` |
| Tray | Icon with **Show** / **Quit**; left-click shows window | `main.rs` setup |
| Close = hide | Window close hides to tray (app keeps running) | `main.rs::on_window_event` |

## 9. Config schema (`AppConfig`, persisted JSON)

| Field | Type | Default | Meaning |
|---|---|---|---|
| `last_sender` | `Option<String>` | none | Sender to auto-reconnect to |
| `output_dir` | path | Videos dir | Where recordings are saved |
| `window_width` / `window_height` | u32 | 1280 / 720 | (declared; not actively used to size the window — `tauri.conf.json` sets 1200×700) |
| `osc_enabled` | bool | true | Start OSC listener |
| `osc_port` | string | "9001" | OSC **listen** port |
| `auto_camera` | bool | true | Send Streaming=true on record start |
| `auto_stop_camera` | bool | true | Send Streaming=false on record stop |
| `vrchat_port` | string | "9000" | OSC **send** port (to VRChat) ⚠ no working UI editor — C2 |
| `video_bitrate` | string | "10M" | ffmpeg `-b:v` ⚠ UI selector doesn't persist it — C1 |
| `mirror_video` | bool | false | Horizontal flip |
| `portrait_mode` | bool | false | 90° rotate |
| `video_vflip` | bool | false | Vertical flip |
| `preview_enabled` | bool | false | Live preview on |
| `mic_enabled` | bool | true | Capture mic |
| `mic_device` | `Option<String>` | none | Selected mic (none = default) |
| `steamvr_overlay` | bool | true | Enable VR overlay |
| `steamvr_auto_launch` | bool | false | Auto-start SteamVR *(in-flight)* |

## 10. IPC surface (Tauri commands — `main.rs`)

`connect_sender(name)`, `disconnect_sender()`, `start_recording()`, `stop_recording()`,
`get_config()`, `update_config(config)`, `poll_events() -> Vec<RenderEvent>`,
`get_stream_port() -> u16`, `list_mic_devices() -> Vec<String>`,
`select_output_dir() -> String`.

> `poll_events` has a side effect: when it dequeues a `ResolutionChanged`, it relaunches the
> process and exits. Auditors should be aware this command is not purely a read.
