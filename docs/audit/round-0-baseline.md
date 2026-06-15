# Round 0 — Baseline Results

Recorded 2026-06-15. Establishes the identified build + reference environment that later rounds
test against. Status legend: ✅ done · ⏳ needs user / separate machine · ⚠ finding.

---

## 1. Build status — ✅ PASS

| Item | Value |
|---|---|
| Command | `cargo build --release` (run from repo root; env vars from `.cargo/config.toml`) |
| Result | **Success, exit 0**, `Finished release profile [optimized] in 2m 42s` |
| Errors | **0** |
| Warnings | 34 (all non-fatal — see §5) |
| Artifact | `target/release/vrc-recorder.exe` |
| Artifact size | **124 MB** — self-contained: embeds `ffmpeg.exe` + 7 av/sw DLLs + `Spout.dll` via `include_bytes!` |
| Toolchain | cargo/rustc **1.96.0** (`ac68faa20`, 2026-05-25), `stable-x86_64-pc-windows-msvc` (edition 2024 OK) |

⚠ **Binary-name mismatch**: `cargo build` produces **`vrc-recorder.exe`** (cargo package name),
but `README.md` says the output is `VRCRecoder.exe` and `tauri.conf.json` `productName` is
`VRCRecorder`. Decide the shipped name / whether distribution uses `cargo build` output vs a
Tauri bundle, and fix the README (which also has the typo "VRCReco**d**er"). Distribution model
today = one ~124 MB self-contained exe.

### Build prerequisites (all satisfied on this machine)
- Spout artifacts present: `vendor/Spout2/cmake-build/bin/Release/Spout.dll` (539 KB),
  `…/lib/Release/Spout.lib` (208 KB), header `…/SPOUTSDK/SpoutGL/SpoutSender.h`. (Built 2026-06-15.)
- VDS ffmpeg present at `C:/Program Files/Virtual Desktop Streamer/` with all 7 required DLLs →
  the build embeds these. **Build will fail to compile without it** (`include_bytes!(env!())`).
- Dep cache warm → build needed **no network** (only compilation).

---

## 2. Embedded binary provenance — ✅ recorded

### FFmpeg (bundled into the exe)
- Version: **`n7.0.2-6-g7e69129d2f` (build 20240831)** — Virtual Desktop Streamer's ffmpeg, gcc 14.2.
- Build config highlights: `--enable-amf --enable-ffnvcodec --enable-libopenh264`,
  **`--disable-libx264 --disable-libx265`**.

### FFmpeg shared DLLs (versions match `bundle.rs` hard-coded names ✓)
`avcodec-61.dll 61.3.100` · `avdevice-61.dll 61.1.100` · `avfilter-10.dll 10.1.100` ·
`avformat-61.dll 61.1.100` · `avutil-59.dll 59.8.100` · `swresample-5.dll 5.1.100` ·
`swscale-8.dll 8.1.100`.

### ⚠ Encoder availability matrix (with THIS bundled ffmpeg)

| App priority (`encoder.rs`) | In bundled ffmpeg? | Note |
|---|---|---|
| `h264_nvenc` | ✅ | **Selected on this machine** (NVIDIA) |
| `h264_amf` | ✅ | AMD path |
| `h264_qsv` | ✅ (string present) | Detection is `contains("h264_qsv")` — matches even where QSV can't init at runtime (fragile) |
| `libx264` | ❌ **disabled in this ffmpeg build** | The app's "software" priority slot is a **dead branch** with this ffmpeg |
| `libopenh264` | ✅ | The **only** true software fallback that will ever be reached |

**Implication for the audit**: a machine with **no hardware encoder** (no NVENC/AMF/QSV) falls
straight to **libopenh264** (slow, lower quality), never libx264. Round 2's "software encoder"
perf test = libopenh264. Also `h264_mf` (MediaFoundation) and `h264_vaapi` exist in the bundle
but the app doesn't consider them.

---

## 3. Reference environment (this dev machine) — ✅ recorded

| Component | Value |
|---|---|
| OS | Windows 11 IoT Enterprise, build 10.0.26200 |
| GPU | **NVIDIA GeForce RTX 4080** (driver 32.0.15.9649) + "Virtual Desktop Monitor" (virtual display) |
| Selected H.264 encoder | **h264_nvenc** |
| Audio devices (names) | USB Audio Device · MOTU M Series · NVIDIA High Definition Audio · Realtek USB Audio · Virtual Desktop Audio |
| Audio sample rates / default in-out | ⏳ capture at runtime from `vrc-recorder.log` (system loopback uses **default output device**; "Virtual Desktop Audio" is a likely default-output candidate) |
| VRChat camera fps setting | ⏳ user to record (critical for the fps/duration question — see `04-observations.md` R1) |

> This is the **development machine**, not a clean machine. The clean-machine acceptance test is
> a separate item (§4).

---

## 4. Open Round 0 items (need user / separate machine) — ⏳

- **R0-A — Reconcile working tree**: ✅ **Done.** Committed as `ec4ea4e` (SteamVR auto-launch +
  cpal/rfd upgrade — the audited code) and `21cfb2e` (audit docs; no binary impact).
  **The audited build corresponds to commit `ec4ea4e`.**
- **R0-B — Smoke test**: ✅ **PASS** (startup + full recording).
  - *Startup* (observed earlier run, `target/release/vrc-recorder.exe`): logging init, MJPEG bound
    (port 59860), OSC bound `127.0.0.1:9001`, `openvr_api.dll` loaded, **OpenGL 4.6 hidden GL
    window created** (riskiest init — passed on the RTX 4080), Spout connected to a live sender
    (`OBS_Spout`) with auto-reconnect observed.
  - *Recording* (`VRCRecorder_2026-06-16_00-05-36.mp4`, 35.5 MB, OBS Spout source): full pipeline
    verified end-to-end — capture → nvenc encode → dual audio → amix merge → OSC stop → file.
    See §5c for analysis.
  - **Still not exercised**: VRChat-specific OSC (orientation follow, `/usercamera/Streaming` —
    source was OBS, not VRChat); fresh extraction from this build's payload (see O1);
    transforms (mirror/portrait/vflip); software/AMD/QSV encoders. These belong to Round 1.
- **R0-D — Runtime audio facts**: ✅ **answered** — system loopback = **96 kHz stereo** (the
  default output device runs at 96 k); mic = **48 kHz stereo** (inferred from byte rate). Output
  AAC = 96 kHz stereo (follows the system source). Feeds R6 / O3.
- **R0-C — Clean-machine run**: on a PC with **no** Rust / SteamVR / VDS installed, does the
  self-contained exe launch, extract its bundled ffmpeg/Spout, and record? (Validates the
  embedding/extraction + delay-load story that store buyers depend on.)
- **R0-D — Runtime audio facts**: from a real recording's log, capture system + mic sample rates
  and channel counts (feeds R6).

---

## 5. Build warnings (non-fatal) — ⚠ for backlog

- **`rust_2024_compatibility` / `unsafe_op_in_unsafe_fn` (E0133, as warnings)** in
  `src/render/preview.rs` (`compile_shader`, `link_program`): unsafe calls inside `unsafe fn`
  without an inner `unsafe {}`. These are **warnings now but become hard errors** in a future
  edition — worth fixing.
- **Dead-code warnings independently confirm `04-observations.md` §D**: unused
  `register_in_appconfig`, `steam_install_path` (steamvr/mod.rs), `PboReader::resize`,
  `GlState::resize` + unread `GlState` fields, `SpoutStream::switch_sender`,
  `RecordingPipeline::is_recording`, `MicCapture::buffer_size`, `RenderCommand::Shutdown` (never
  constructed → there is no graceful render-thread shutdown path), and several unused
  OpenVR fn-table consts/methods.
- Several benign `unused_mut` / `unused_unsafe`.
- Cleanest path: `cargo fix` for the mechanical ones; review dead code against intent before
  deleting (some, like `RenderCommand::Shutdown`, point at missing functionality rather than
  removable code).

---

## 5b. Runtime findings (from the smoke test) — ⚠

- **O1 🟠 (release-critical for updates) — stale bundled payload on upgrade.**
  `bundle::extract()` writes each file only `if !path.exists()`, so once a user has run any
  version, the **embedded ffmpeg / DLLs / Spout.dll are never redeployed on upgrade**. Proven
  here: the freshly built exe embeds Spout.dll `539,136 B` (md5 `fc758f26…`) but the run loaded
  the **May-18 cached** `533,504 B` (md5 `616fda29…`). ffmpeg happened to be byte-identical
  (md5 `13aa0165…`), masking the issue for that file. → Shipping a fixed ffmpeg/Spout in a future
  version won't reach existing users; risks silent version mismatch. Fix: version the extraction
  (e.g. embed a payload hash/version and overwrite when it differs). Cross-ref `04-observations.md` O1.
  **Audit methodology note**: clear `%LOCALAPPDATA%/vrc-recorder/` between builds when testing
  bundled-dependency changes, or you'll test stale binaries.

- **O2 🟡 — SteamVR overlay busy-retry log spam.** With `steamvr_overlay=true` (default) and no
  SteamVR/HMD, the overlay thread logs `VR_InitInternal failed (error=108) — retrying in 3s`
  **every 3 s forever** (the observed log is almost entirely this). Benign functionally, but a
  desktop-only user's log grows unbounded and a thread wakes every 3 s indefinitely. Consider
  backing off the retry interval and/or rate-limiting the log line. Cross-ref `04-observations.md` O2.

## 5c. Recording analysis (first real output) — ✅ + early R1/R2/R6 data

Sample: `VRCRecorder_2026-06-16_00-05-36.mp4`, source = OBS Spout @ 1080p, ~31 s, nvenc.

| Metric | Value | Read |
|---|---|---|
| Video | h264 1920×1080, `avg_frame_rate=60/1`, **1881 frames**, 9.3 Mbps | matches 10M target (nvenc VBR) |
| Dropped frames | **0** (per log, sustained) | nvenc has ample headroom at 1080p60 |
| Video duration | 31.350 s (= 1881 / 60) | duration is exactly frame_count/60 by construction |
| Real capture rate | **~60.3 fps** (1440 frames over 23.88 s of log timestamps) | source was genuinely ~60 → the fixed-60 assumption **held here** |
| Audio | AAC **96 kHz** stereo, 31.264 s, 195 kbps | output rate follows the 96 kHz system device (see O3) |
| Sys + mic | system 96 kHz + mic 48 kHz both captured, `amix` merged OK | **R6 core question answered: mismatched rates work** (ffmpeg auto-resamples); surround still untested |
| A/V end delta | video 31.350 − audio 31.264 = **+86 ms** (video longer) | expected: audio shorter → silent tail; imperceptible. No mid-stream drift (0 drops) |
| Finalize | nvenc speed=1x, `Streaming false` OSC sent on stop, file saved | clean stop path |

**Interpretation**: the recording pipeline is **correct and performant for the favorable case**
(source ≈ 60 fps, HW encoder, no drops). This does **not** retire R1/R2 — the open risk is
**non-60 / variable / GPU-bound sources** (e.g. VRChat camera at 30/72/90), where duration and
sync would diverge. It's now a measured baseline to compare those against. ⚠ Visual playback /
audio audibility to be confirmed by a human (ffprobe validates structure, not content).

## 6. Round 0 verdict

- **Build**: reproducible and clean (0 errors) on the dev machine → **baseline build established**.
- **Identified baseline**: audited code = commit **`ec4ea4e`** (docs at `21cfb2e`). ✅
- **Provenance & environment**: documented above; pin these in any bug report.
- **Smoke test**: ✅ startup + full recording verified (§5b/§5c); R0-D answered.
- **Remaining for full Round 0 sign-off**: only **R0-C (clean-machine run)** — needs a separate
  PC with no Rust/SteamVR/VDS, recommended before release since the embed/extract model is
  untested there (and O1 means stale payloads can hide on dev machines).
- **Round 0 outcome**: baseline build + environment **established and identified**; core record
  path **proven** on the reference machine. Ready to proceed to Round 1 (correctness), carrying
  the open items O1, O2, O3 and the R1/R2 non-60 fps question.
