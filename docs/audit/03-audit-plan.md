# 03 — Audit Plan & Checklists

Three rounds, one per focus area. Each round: run the checklist, record pass/fail + evidence
(log excerpts, sample files, screen captures), and file divergences against
`02-functional-spec.md`. Start from the pre-identified items in `04-observations.md`.

## Round 0 — Establish a stable baseline (do this first)

- [ ] Reconcile the **uncommitted working tree** (cpal/rfd bump + SteamVR auto-launch). Commit or stash so the audited build is identified by a commit.
- [ ] Confirm a clean **release build** on a clean machine: Spout SDK built, VDS ffmpeg present (build embeds it), env vars set. Record exact `ffmpeg -version` / DLL versions embedded.
- [ ] Stand up a **manual smoke-test** path (no automated tests exist): launch → connect sender → preview → record 30 s → verify file opens with audio.
- [ ] Capture a **clean-machine** run: no Rust, no SteamVR, no VDS installed — does the packaged app run and record? (ffmpeg/Spout are embedded; confirm extraction works.)
- [ ] Define the **reference environment(s)**: GPU vendor (NVENC vs AMF vs QSV vs software), audio device sample rates, VRChat camera fps setting.

---

## Round 1 — Functional correctness

### Recording integrity
- [ ] Record 30 s / 5 min / 30 min / 2 h. Each file plays start-to-end in VLC **and** Windows Media Player / browser.
- [ ] **A/V sync** measured at start, middle, end (clap test or slate). Quantify drift (ms) vs duration. *(R1/R2)*
- [ ] **Duration accuracy**: file duration vs wall-clock recording time, across encoders and source fps. *(R1)*
- [ ] Confirm actual **Spout delivery fps** vs the hard-coded 60 (instrument frame timestamps). Test with VRChat camera at 30/60/72/90/144 and when GPU-bound. *(R1)*
- [ ] Frame-drop behavior: force a slow encoder (software) at high res; verify dropped-frame count, and whether drops cause desync or just shortened video. *(R2/R7)*
- [ ] **No black/duplicate tail frames** at end (the flush + dedup logic in `capture.rs::flush`).
- [ ] First-frame correctness (PBO pipeline only returns data from frame 2 onward).

### Audio correctness
- [ ] System-only, mic-only, both, and neither (loopback fails) — each produces a valid file.
- [ ] Mix levels audible and balanced (sys 1.0 / mic 0.5). *(U5)*
- [ ] **Mic selection is honored for the whole recording** — select a non-default mic, record 1 min, confirm it doesn't silently switch to default at ~2 s. *(C3 — high priority)*
- [ ] Sample-rate / channel combos: 44.1k mono mic + 48k stereo output; **surround (5.1/7.1) output device**. Verify mix is correct and in sync. *(R6)*
- [ ] Mic unplugged / default device changed mid-recording — graceful behavior.
- [ ] Audio length vs video: confirm `-t frame_count/60` trimming doesn't clip wanted audio or leave long silence.

### Transforms
- [ ] All 8 mirror×portrait×vflip combinations: **preview matches the recorded file** orientation exactly. *(R8)*
- [ ] Portrait output dimensions correct (W/H swapped) in both preview and file.

### Config & state
- [ ] **Bitrate selector actually changes output bitrate** (probe file bitrate for 4/10/20/40M). *(C1 — high priority)*
- [ ] **VRChat send port editable** and used; bitrate not corrupted by editing it. *(C2 — high priority)*
- [ ] Config survives restart; corrupt/old config falls back to defaults without crashing (add/remove fields).
- [ ] Output-dir change takes effect for the next recording; non-existent / read-only / full disk handled with a clear error. *(U6)*
- [ ] Disconnect actually stays disconnected (or document that auto-connect resumes). *(C4)*

### Integrations
- [ ] OSC orientation follow: toggling VRChat camera landscape/portrait flips the recording.
- [ ] Auto Spout on/off OSC actually toggles VRChat's stream camera (correct address/port).
- [ ] SteamVR overlay: appears in dashboard; controller `toggle_record` starts/stops; texture flips READY↔RECORDING.
- [ ] SteamVR auto-launch: off = never launches; on = launches once; live toggle while waiting. *(in-flight feature)*
- [ ] Interface-version resilience: behavior if installed SteamVR exposes a different `IVROverlay_0xx` than `_027`. *(R-VR)*

### Robustness / failure modes
- [ ] **Resolution change mid-recording**: confirm whether the relaunch path corrupts/loses the in-progress recording (background merge thread killed by `process::exit`). *(R4 — high priority)*
- [ ] **Abrupt termination** (kill, crash, power loss) during recording: is the MP4 recoverable? *(R5)*
- [ ] ffmpeg process killed externally mid-recording — is it surfaced to the user? *(R7)*
- [ ] Encoder write error mid-recording — UI must not keep showing "recording" with no output. *(R7)*
- [ ] Two recordings stopping within the same second — filename collision/overwrite. *(U7)*
- [ ] Start/stop spammed rapidly — no deadlock, no leaked ffmpeg processes.

---

## Round 2 — Performance

- [ ] **Idle** (connected, no preview, no record): CPU%, GPU%, the 16 ms render-loop wake cost, the 11 ms SteamVR poll cost.
- [ ] **Preview on**: cost of per-frame synchronous `glReadPixels` + JPEG q80 encode on the render thread at 1080p/1440p/4K; does it cap the capture rate? *(perf)*
- [ ] **Recording**: end-to-end CPU/GPU per encoder (NVENC/AMF/QSV/x264/openh264) at 1080p60 / 1440p60 / 4K60. Identify where software encoders fall behind → drops.
- [ ] **RAM over time**: audio buffers grow unbounded in RAM for the whole recording (~0.7 GB/h per stream at 48k stereo). Measure RSS at 1 h / 2 h / 4 h; find the practical max recording length before pressure. *(R3 — design concern)*
- [ ] Frame-channel backpressure: capacity-60 (~500 MB at 1080p) — peak RAM during encoder stalls.
- [ ] Disk throughput: intermediate video + PCM temp files; behavior on slow/HDD targets.
- [ ] Startup time and extraction cost (ffmpeg + DLLs written to cache on first run).
- [ ] Thread/handle growth across many record cycles (stderr reader thread is `mem::forget`-ed each time). *(leak check)*
- [ ] MJPEG preview bandwidth/CPU with the WebView client attached.

---

## Round 3 — Interface / UX

### Functional UI
- [ ] Every control persists and round-trips through `config.json` (systematically click each).
  - Known broken: **bitrate selector** *(C1)*, **send-port field** *(C2)*. Re-verify after fix.
- [ ] Toggle visual state always matches actual config (including OSC-driven orientation changes).
- [ ] Status line conveys real state; errors are visible (not only in the log file). *(R7)*

### Clarity / discoverability
- [ ] First-run experience without VRChat/SteamVR open: is "No senders / No Signal" understandable?
- [ ] **Where did my recording go?** No save confirmation or "open folder" today. *(U1)*
- [ ] **How do I quit?** Close hides to tray; quit only via tray. Verify users can find it. *(U2)*
- [ ] Preview auto-disables on record with no explicit explanation — is that surprising?
- [ ] Bitrate/format guidance — do users understand 4M vs 40M tradeoffs?

### Store readiness (booth.pm / general)
- [ ] **Localization**: UI is English-only; target marketplace is Japanese. Decide on JP (and the SteamVR overlay font only supports A–Z/0–9). *(U3)*
- [ ] Offline behavior: Google Fonts loaded from the network → fallback font when offline; consider bundling fonts. *(U4)*
- [ ] Branding/version/about, license attributions (FFmpeg LGPL/GPL, Spout, OpenVR) present and correct for redistribution.
- [ ] Installer/packaging story (currently a bare `.exe`); SmartScreen/AV implications of an unsigned exe that extracts + delay-loads DLLs and shells out to ffmpeg.
- [ ] Permissions/privacy disclosure: app captures system audio + mic + screen-camera; document it.
- [ ] CSP is disabled (`csp: null`) — review for store/security expectations. *(U8)*

---

## Evidence & reporting convention

For each failed/uncertain item, capture: **what was expected** (cite `02-functional-spec.md`),
**what happened**, **evidence** (log lines from `%LOCALAPPDATA%/vrc-recorder/vrc-recorder.log`,
sample output file, `ffprobe` output, screenshot), **env** (GPU/encoder, audio devices, source
fps), and a link to the matching `04-observations.md` ID (or a new one).
