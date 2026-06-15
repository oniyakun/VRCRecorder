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

- **R0-A — Reconcile working tree**: HEAD is `0804bd2`; 10 tracked files are modified + `docs/`
  untracked. The audited build does **not** yet map to a commit. *Decision pending* (commit
  strategy).
- **R0-B — Interactive smoke test**: launch → connect a live VRChat Spout sender → enable preview
  → record 30 s → confirm the output MP4 plays with video **and** audio. Needs VRChat + a human.
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

## 6. Round 0 verdict

- **Build**: reproducible and clean (0 errors) on the dev machine → **baseline build established**.
- **Provenance & environment**: documented above; pin these in any bug report.
- **Blocking for "stable identified baseline"**: R0-A (commit). **Recommended**: also do R0-C
  (clean machine) before trusting the distribution before release, since the whole embed/extract
  model is untested here.
