# 04 — Pre-Audit Observations & Risk Register

Candidate issues found while reading the source (current working copy). Each has a **severity**,
a **confidence** (how sure it's a real problem from code alone), `file:line` references, and a
verification/fix direction. **Confided-from-code** items can feed a fix backlog after a quick
confirm; **Risk** items need runtime verification in the relevant audit round.

Severity: 🔴 high · 🟠 medium · 🟡 low/cosmetic.
Confidence: ✅ confirmed from code · 🔶 likely, verify · ◻️ open question.

---

## A. Confirmed-from-code bugs (frontend/config wiring)

### C1 🔴 ✅ — Bitrate selector never persists
`frontend/index.html:195-200` defines `#bitrate-select` (4M/10M/20M/40M). `frontend/main.js`
sets its value on init (`main.js:304`) but **registers no `change` handler** for it. Selecting a
bitrate updates neither `state.config` nor the backend. Output is always the saved/default
`video_bitrate` ("10M").
→ **Fix**: add a `change` listener that sets `state.config.video_bitrate` and calls
`update_config`; also update the `#meta-bitrate` readout.

### C2 🔴 ✅ — "Send port (to VRChat)" field writes to the wrong config key
`frontend/main.js:269-273`: the `#osc-send-port` `change` handler sets
`state.config.video_bitrate = this.value`. It should set `vrchat_port`. Two consequences:
1. Editing the send-port field **corrupts `video_bitrate`** (e.g. to `"9000"` → ffmpeg
   `-b:v 9000` ≈ 9 kbit/s).
2. `vrchat_port` has **no working UI editor** at all (it is otherwise only set by default/JSON).
→ **Fix**: `state.config.vrchat_port = this.value`. (Pairs with C1, which is the *real* bitrate control.)

### C3 🔴 ✅ — A non-default microphone is silently reset to the system default ~2 s into recording
During recording, `render_thread.rs:515-524` calls `RecordingPipeline::check_mic_device` every 2 s.
`check_mic_device` (`record/pipeline.rs:246-260`) calls `MicCapture::device_changed`, and on
"changed" **restarts capture with `MicCapture::start(None)`** — i.e. the *default* device,
ignoring the user's selection. `device_changed` (`record/mic.rs:89-97`) compares the **default
input device** name to the currently-captured device name. If the user picked a *non-default*
mic, these never match, so `device_changed` returns `true` on the first check and capture is
switched to the default mic.
→ Net effect: selecting any non-default mic records the wrong device after ~2 s.
→ **Fix**: compare against the *configured* device and restart with the configured device, not
`None`; only treat "default device changed" as relevant when `mic_device` is `None`.

### C4 🟠 ✅ — "Disconnect" is undone by auto-connect within 2 s
`disconnect_sender` (`render_thread.rs:198-211`) sets `spout_stream = None`. But `scan_senders`
(`render_thread.rs:543-554`) auto-connects whenever `spout_stream.is_none()` and any sender
exists — so the next 2 s scan reconnects (to `last_sender`, which `disconnect` cleared, then the
first sender). Manual disconnect therefore doesn't stick while a sender is live.
→ **Fix/decide**: add an explicit "user disconnected" state that suppresses auto-connect until
the user reconnects, or remove the Disconnect affordance, or document the behavior.

---

## B. Correctness risks (verify at runtime — Round 1)

### R1 🔴 🔶 — Fixed 60 fps assumption drives duration & speed
The encoder is hard-coded to 60 fps input (`pipeline.rs:74` `60.0`; `encoder.rs:93-101,132`
`-r 60`) and audio is aligned to `frame_count/60.0` (`pipeline.rs:217`). Capture only happens on
new Spout frames at a ≤60 Hz poll. If VRChat/Spout delivers frames at a rate other than exactly
60 (camera fps setting, GPU-bound, vsync), the **output plays too fast/slow** and **duration is
wrong**. There is no measurement of the actual inter-frame time and no use of `-vsync`/PTS.
→ **Verify**: instrument real delivery fps; compare file duration to wall-clock across source fps.
→ **Likely fix direction**: timestamp frames and either set the real input rate or feed PTS
(`-vsync vfr` / `-use_wallclock_as_timestamps`), instead of assuming 60.

### R2 🔴 🔶 — A/V sync drift between wall-clock audio and frame-counted video
Audio is captured in real time (WASAPI callbacks). Video length is derived from frame **count**
at 60 fps. Steady-state frame **drops** (channel cap 60, `encoder.rs:172`; non-blocking
`push_frame`, `encoder.rs:211-229`) reduce `frame_count`, so video gets shorter than the
real elapsed time while audio stays real-time → **progressive desync**, with the tail hidden by
the `-t frame_count/60` trim. Worst on software encoders / high resolution.
→ **Verify**: slate/clap test at start/mid/end under induced drops; quantify drift.

### R3 🟠 🔶 — Whole-recording audio is buffered in RAM
`AudioCapture`/`MicCapture` append PCM to an in-memory `Vec<u8>` for the **entire** recording and
only drain at stop (`record/audio.rs:41-103`, `record/mic.rs:50-110`, `pipeline.rs:121-146`).
At 48 kHz stereo s16 that's ~0.69 GB/hour **per stream** (system + mic ≈ 1.4 GB/h). Multi-hour
streams risk high RAM / allocation stalls / OOM. Video, by contrast, streams to disk via ffmpeg.
→ **Verify**: RSS over 1/2/4 h. **Fix direction**: stream PCM to the temp `.pcm` files
incrementally instead of holding it all in memory.

### R4 🔴 🔶 — Resolution change mid-recording likely corrupts the output
On a resolution change while recording, `update_spout` (`render_thread.rs:343-368`) stops
recording — which spawns the **background `encoder-finish`/merge thread** (`pipeline.rs:194-241`)
— then emits `ResolutionChanged`. ~300 ms later the UI's `poll_events` (`main.rs:131-145`)
**relaunches the process and calls `process::exit(0)`**, killing that background thread before
ffmpeg finishes/merges. For anything but a tiny recording, the in-progress file is likely
truncated/unmerged/lost.
→ **Verify**: change camera resolution mid-recording; inspect the resulting file.
→ **Fix direction**: join the finish thread (or finalize synchronously) before relaunch, or avoid
the full-process relaunch entirely.

### R5 🟠 🔶 — No crash-safe finalization (MP4 moov-at-end)
The MP4 is finalized only when ffmpeg's stdin closes cleanly. Abrupt termination (crash, the R4
relaunch, power loss, killed ffmpeg) leaves an unfinalized, likely **unplayable** file. No
`-movflags +faststart`/fragmented-mp4 or recovery path.
→ **Verify**: kill the app mid-recording; attempt playback/repair.

### R6 🟠 🔶 — Mixed sample rates / surround channel layouts in the audio merge
System audio uses the output device's native rate & channel count (could be 48 k 5.1/7.1); mic
may be 44.1 k mono. `amix` (`pipeline.rs:399`) relies on ffmpeg's implicit format negotiation.
Verify the mix is correct, in sync, and that surround outputs don't produce a wrong/garbled
downmix or channel-count mismatch.
→ **Verify**: test 5.1 output device + mono mic; check channels, levels, sync.

### R7 🟠 🔶 — Mid-recording encoder failure isn't surfaced
If `push_frame` returns `Err` (writer thread/ffmpeg gone), `process_frame` only logs it
(`pipeline.rs:296-298`) and `self.recording` stays `true`. The UI keeps showing "REC" while no
usable output is produced. Likewise `RecordingStopped.final_path` is always `None`
(`render_thread.rs:286-288`), so there's never a success/failure signal to the user.
→ **Fix direction**: detect encoder death, emit an `Error`/auto-stop, and populate `final_path`.

### R8 🟠 🔶 — Preview vs recording orientation may not match
Preview applies transforms via GPU UV math (`render/preview.rs:138-170`); recording applies them
via ffmpeg filters (`encoder.rs:106-123`). The preview comment references `transpose=1` while the
encoder uses `transpose=2` (`preview.rs:139` vs `encoder.rs:108`). The two paths must agree for
all 8 mirror×portrait×vflip combinations (WYSIWYG).
→ **Verify**: side-by-side preview vs recorded file for every combination.

### R-VR 🟡 🔶 — Hard-coded OpenVR interface versions & fn-table indices
`steamvr/ffi.rs:79-81` pins `IVROverlay_027` / `IVRInput_010` / `IVRApplications_007` and uses
hard-coded fn-table indices (`ffi.rs:56-76`). A SteamVR update exposing different versions/layout
would break the overlay (silent `get_interface` failure → thread returns). Acceptable risk, but
note it and fail loudly/gracefully.

---

## C. UX / store-readiness gaps (Round 3)

- **U1 🟠** No "saved to …" confirmation or "open folder" action; `RecordingStopped.final_path`
  is never populated (`render_thread.rs:286`, `channels.rs:43-45`).
- **U2 🟡** Window close hides to tray (`main.rs:357-361`); quit only via tray **Quit** — easy to
  miss.
- **U3 🟠** UI is English-only; target market is booth.pm (JP). SteamVR overlay font supports only
  A–Z/0–9 (`steamvr/texture.rs:62-102`). Plan localization.
- **U4 🟡** Google Fonts loaded from the network (`frontend/index.html:7-9`); offline users get a
  fallback font. Consider bundling.
- **U5 🟡** Mix levels hard-coded (system 1.0 / mic 0.5, `pipeline.rs:399`); no UI control.
- **U6 🟠** No disk-space / write-permission preflight; failures only show in the status line.
- **U7 🟡** Final filename uses the **stop-time** timestamp (`pipeline.rs:214-215`), differing
  from the intermediate's start-time name; two stops in the same second collide (merge uses `-y`).
- **U8 🟡** `csp: null` disables CSP (`tauri.conf.json:18`); fine for a local app but note for
  store/security review.

---

## D. Minor / housekeeping

- **Dead code**: `PboReader::resize` (`record/capture.rs:112-128`) is never called (resolution
  change relaunches instead); `steamvr/mod.rs::register_in_appconfig` (`:431-494`) and
  `steam_install_path` there appear unused (registration goes via `IVRApplications`).
- **Confused-but-harmless**: preview cap `let max_dim = 1920u32.max(1080u32)` = 1920
  (`render_thread.rs:409`); comment says "1080p". Works, but misleading.
- **Resource**: ffmpeg stderr reader thread is `mem::forget`-ed every recording
  (`encoder.rs:197-199`) — check handle growth over many cycles (R-perf).
- **Build coupling**: building **requires** VDS's `ffmpeg.exe` present at compile time (embedded
  via `include_bytes!(env!("FFMPEG_EXE_PATH"))`, `bundle.rs:4`); otherwise compilation fails.
  Document/relax for reproducible builds.
- **In-flight work**: cpal `0.15→0.18` and rfd `0.15→0.17` bumps changed the audio device API
  (`record/audio.rs`, `record/mic.rs`) — audio is a freshly-touched area; weight Round 1 audio
  tests accordingly.

---

## Suggested triage order

1. **C1, C2** — trivial frontend fixes; core controls currently broken. (Highest value/effort.)
2. **C3** — wrong-mic recording; small backend fix.
3. **R1/R2** — the central correctness question (fps/duration/sync); needs measurement first.
4. **R4** — potential data loss on resolution change.
5. **R3, R5, R7** — robustness/scale for long sessions.
6. **R6, R8, R-VR** — verify; fix if confirmed.
7. **U1–U8** — UX/store polish for release.
