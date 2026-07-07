use std::path::PathBuf;
use std::os::windows::process::CommandExt;
use std::time::Instant;

use crate::record::audio::AudioCapture;
use crate::record::capture::PboReader;
use crate::record::encoder::FfmpegEncoder;
use crate::record::mic::MicCapture;

pub struct RecordingPipeline {
    encoder: Option<FfmpegEncoder>,
    pbo_reader: Option<PboReader>,
    audio_capture: Option<AudioCapture>,
    mic_capture: Option<MicCapture>,
    output_dir: PathBuf,
    frame_count: u64,
    dropped_frames: u64,
    /// Path where the encoded .mp4 will be written (used by finish step).
    video_path: Option<PathBuf>,
    /// Frame-pacing diagnostics, present only while recording.
    timing: Option<FrameTiming>,
}

impl RecordingPipeline {
    pub fn new(output_dir: PathBuf) -> Self {
        Self {
            encoder: None,
            pbo_reader: None,
            audio_capture: None,
            mic_capture: None,
            output_dir,
            frame_count: 0,
            dropped_frames: 0,
            video_path: None,
            timing: None,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.encoder.is_some()
    }

    pub fn set_output_dir(&mut self, dir: PathBuf) {
        self.output_dir = dir;
    }

    pub fn start(
        &mut self,
        gl: &glow::Context,
        width: u32,
        height: u32,
        fps: f64,
        bitrate: &str,
        mirror: bool,
        portrait: bool,
        vflip: bool,
        mic_enabled: bool,
        mic_device: Option<&str>,
    ) -> Result<(), String> {
        if self.encoder.is_some() {
            return Err("Already recording".into());
        }

        let ffmpeg_path = find_ffmpeg()?;

        std::fs::create_dir_all(&self.output_dir)
            .map_err(|e| format!("Failed to create output dir: {e}"))?;

        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
        let video_path = self.output_dir.join(format!(
            "VRCRecorder_{timestamp}_video.mp4"
        ));

        let encoder = FfmpegEncoder::start(
            &ffmpeg_path,
            &video_path,
            width,
            height,
            fps,
            bitrate,
            mirror,
            portrait,
            vflip,
        )?;

        let pbo = PboReader::new(gl, width, height);

        // Audio PCM is streamed to temp files alongside the intermediate video,
        // not buffered in RAM (see P2). Paths derive from the video path.
        let sys_pcm_path = video_path.with_extension("sys.pcm");
        let mic_pcm_path = video_path.with_extension("mic.pcm");

        let audio = match AudioCapture::start(sys_pcm_path) {
            Ok(a) => {
                log::info!("System audio capture started");
                Some(a)
            }
            Err(e) => {
                log::warn!("Audio capture unavailable: {e}. Recording video only.");
                None
            }
        };

        let mic = if mic_enabled {
            match MicCapture::start(mic_device, mic_pcm_path) {
                Ok(m) => {
                    log::info!("Mic capture started: {}", m.device_name());
                    Some(m)
                }
                Err(e) => {
                    log::warn!("Mic capture unavailable: {e}");
                    None
                }
            }
        } else {
            None
        };

        self.encoder = Some(encoder);
        self.pbo_reader = Some(pbo);
        self.audio_capture = audio;
        self.mic_capture = mic;
        self.video_path = Some(video_path);
        self.frame_count = 0;
        self.dropped_frames = 0;
        self.timing = Some(FrameTiming::new());

        log::info!("Recording started: {}x{}", width, height);
        Ok(())
    }

    pub fn stop(
        &mut self,
        gl: &glow::Context,
        spout_tex: Option<glow::Texture>,
        wall_elapsed_secs: Option<f64>,
    ) -> Result<PathBuf, String> {
        // Let the last audio callbacks land, then stop the streams and finalize
        // the temp PCM files (the writer threads flush + close on stop()).
        std::thread::sleep(std::time::Duration::from_millis(50));

        let audio_data = if let Some(ref mut audio) = self.audio_capture {
            let bytes = audio.stop();
            log::info!("System audio captured: {bytes} bytes");
            if bytes > 0 {
                Some((audio.pcm_path().clone(), audio.sample_rate(), audio.channels()))
            } else {
                None
            }
        } else {
            None
        };

        let mic_data = if let Some(ref mut mic) = self.mic_capture {
            let bytes = mic.stop();
            log::info!("Mic audio captured: {bytes} bytes");
            if bytes > 0 {
                Some((mic.pcm_path().clone(), mic.sample_rate(), mic.channels()))
            } else {
                None
            }
        } else {
            None
        };

        // Flush remaining PBO frames + synchronous read of current Spout
        // texture to ensure the latest frame is captured. With no texture
        // (e.g. source never connected) there is nothing to flush — we still
        // fall through and finalize the encoder below.
        let mut flush_count = 0u64;
        if let Some(tex) = spout_tex {
            if let Some(ref mut pbo) = self.pbo_reader {
                for data in pbo.flush(gl, tex) {
                    if let Some(ref mut encoder) = self.encoder {
                        // Blocking send — guarantees final frames are never dropped
                        if let Err(e) = encoder.push_frame_blocking(data) {
                            log::error!("Encoder error during final flush: {e}");
                        } else {
                            self.frame_count += 1;
                            flush_count += 1;
                        }
                    }
                }
            }
        }
        if flush_count > 0 {
            log::info!("Flushed {flush_count} final frames");
        }

        // One-time frame-pacing summary (concise; see FrameTiming). Emitted
        // before frame_count is reset below.
        if let Some(t) = self.timing.take() {
            t.log_summary(self.frame_count, wall_elapsed_secs);
        }

        // Take ownership for background finalization.
        let mut encoder = self.encoder.take();
        let video_path = self.video_path.take();
        let frame_count = self.frame_count;
        let output_dir = self.output_dir.clone();

        // Clean up GPU resources on render thread
        if let Some(pbo) = self.pbo_reader.take() {
            pbo.destroy(gl);
        }

        // Reset state immediately so a new recording can start
        self.frame_count = 0;
        self.dropped_frames = 0;
        self.audio_capture = None;
        self.mic_capture = None;

        // Compute the expected final output path now so we can return it to the
        // frontend immediately. The background thread merges into the same path.
        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
        let final_output_path = output_dir.join(format!("VRCRecorder_{timestamp}.mp4"));

        // Finish ffmpeg encoding + merge audio on background thread.
        if let Some(mut enc) = encoder {
            let final_path_for_thread = final_output_path.clone();
            std::thread::Builder::new()
                .name("encoder-finish".into())
                .spawn(move || {
                    // Step 1: close pipe, wait for ffmpeg to finish encoding
                    log::info!("Finishing video: {} frames encoded", frame_count);
                    if let Err(e) = enc.finish() {
                        log::error!("Encoder finish failed: {e}");
                        return;
                    }

                    // Step 2: merge audio into final .mp4
                    if let Some(ref vp) = video_path {
                        let ffmpeg_path = match find_ffmpeg() {
                            Ok(p) => p,
                            Err(e) => {
                                log::error!("{e}");
                                return;
                            }
                        };
                        let final_path = final_path_for_thread;

                        // Re-time the 60fps-encoded video to the measured capture
                        // rate so file duration matches wall-clock and audio stays
                        // in sync regardless of the real Spout delivery rate (P1).
                        let (itsscale, video_duration) =
                            retime_factor(frame_count, wall_elapsed_secs);
                        match merge_audio_video(
                            &ffmpeg_path,
                            vp,
                            audio_data,
                            mic_data,
                            &final_path,
                            video_duration,
                            itsscale,
                        ) {
                            Ok(()) => {
                                let _ = std::fs::remove_file(vp);
                                log::info!("Recording saved to {}", final_path.display());
                            }
                            Err(e) => {
                                log::error!("Audio merge failed: {e}. Keeping video-only file.");
                                let _ = std::fs::remove_file(&final_path);
                                if std::fs::rename(vp, &final_path).is_ok() {
                                    log::info!("Video-only recording saved to {}", final_path.display());
                                }
                            }
                        }
                    }
                })
                .map_err(|e| format!("Failed to spawn finish thread: {e}"))?;
        }

        Ok(final_output_path)
    }

    pub fn process_frame(&mut self, gl: &glow::Context, spout_tex: glow::Texture) {
        let (pbo, encoder) = match (&mut self.pbo_reader, &mut self.encoder) {
            (Some(pbo), Some(enc)) => (pbo, enc),
            _ => return,
        };

        pbo.begin_frame(gl, spout_tex);
        if let Some(rgba_data) = pbo.end_frame(gl) {
            match encoder.push_frame(rgba_data) {
                Ok(true) => {
                    self.frame_count += 1;
                    if let Some(t) = &mut self.timing {
                        t.on_push();
                    }
                }
                Ok(false) => {
                    self.dropped_frames += 1;
                    if let Some(t) = &mut self.timing {
                        t.on_drop();
                    }
                }
                Err(e) => {
                    log::error!("Encoder write error on frame {}: {e}", self.frame_count);
                }
            }
        }
    }
}

impl Drop for RecordingPipeline {
    fn drop(&mut self) {
        drop(self.encoder.take());
        if let Some(ref mut audio) = self.audio_capture {
            audio.stop();
        }
    }
}

// ───── Frame-pacing diagnostics (stutter detection) ─────
//
// Accumulates integer counters only — no per-frame log lines or allocations.
// During recording it stays SILENT unless an interval actually had hitches or
// dropped frames; at stop it emits one concise summary plus an inter-frame
// histogram. This keeps the log near-empty for smooth recordings (drive-space
// friendly for normal users) while capturing exactly what's needed to diagnose
// a reported stutter from a user's log file. Timing uses `Instant`, which is
// QPC-backed on Windows — the monotonic clock the research recommends.

/// Inter-frame gap (ms) above which a frame counts as a "hitch" (≈1.5× a 60fps
/// frame — at least one frame's worth of stall).
const HITCH_MS: u64 = 25;
/// Cadence of the problem-only interval report.
const REPORT_SECS: u64 = 10;

struct FrameTiming {
    start: Instant,
    last_push: Option<Instant>,
    /// Inter-frame interval histogram, buckets in ms:
    /// [<20, 20–33, 33–50, 50–100, 100–250, >250].
    buckets: [u64; 6],
    hitches: u64,
    drops: u64,
    max_gap_ms: u64,
    // Per-interval accumulators (reset on each report).
    interval_start: Instant,
    interval_pushed: u64,
    interval_hitches: u64,
    interval_drops: u64,
}

impl FrameTiming {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last_push: None,
            buckets: [0; 6],
            hitches: 0,
            drops: 0,
            max_gap_ms: 0,
            interval_start: now,
            interval_pushed: 0,
            interval_hitches: 0,
            interval_drops: 0,
        }
    }

    /// A frame was successfully handed to the encoder.
    fn on_push(&mut self) {
        let now = Instant::now();
        if let Some(prev) = self.last_push {
            let ms = now.duration_since(prev).as_millis() as u64;
            let b = match ms {
                0..=19 => 0,
                20..=32 => 1,
                33..=49 => 2,
                50..=99 => 3,
                100..=249 => 4,
                _ => 5,
            };
            self.buckets[b] += 1;
            if ms > HITCH_MS {
                self.hitches += 1;
                self.interval_hitches += 1;
            }
            if ms > self.max_gap_ms {
                self.max_gap_ms = ms;
            }
        }
        self.last_push = Some(now);
        self.interval_pushed += 1;
        self.maybe_report(now);
    }

    /// The encoder channel was full and a frame was dropped.
    fn on_drop(&mut self) {
        self.drops += 1;
        self.interval_drops += 1;
    }

    /// Emit an interval line only when the interval actually had a problem, so a
    /// smooth recording writes nothing here.
    fn maybe_report(&mut self, now: Instant) {
        let secs = now.duration_since(self.interval_start).as_secs();
        if secs < REPORT_SECS {
            return;
        }
        if self.interval_hitches > 0 || self.interval_drops > 0 {
            let fps = self.interval_pushed as f64 / secs as f64;
            let at = now.duration_since(self.start).as_secs();
            log::warn!(
                "Pacing@{at}s: {fps:.0} fps, {} hitches, {} encoder-drops over {secs}s",
                self.interval_hitches,
                self.interval_drops,
            );
        }
        self.interval_start = now;
        self.interval_pushed = 0;
        self.interval_hitches = 0;
        self.interval_drops = 0;
    }

    /// One-time summary at stop: real fps, drop %, hitches, worst gap, the
    /// inter-frame histogram, and a best-effort capture-vs-encoder attribution.
    fn log_summary(&self, frame_count: u64, wall_secs: Option<f64>) {
        let wall = wall_secs.unwrap_or(0.0);
        let real_fps = if wall > 0.1 { frame_count as f64 / wall } else { 0.0 };
        let total = frame_count + self.drops;
        let drop_pct = if total > 0 {
            self.drops as f64 * 100.0 / total as f64
        } else {
            0.0
        };
        let b = self.buckets;
        log::info!(
            "Recording stats: {frame_count} frames / {wall:.1}s = {real_fps:.1} real fps; \
             encoder drops {} ({drop_pct:.1}%); hitches(>{HITCH_MS}ms) {}, worst gap {}ms",
            self.drops,
            self.hitches,
            self.max_gap_ms,
        );
        log::info!(
            "Frame-interval histogram ms [<20|20-33|33-50|50-100|100-250|>250]: {} {} {} {} {} {}",
            b[0], b[1], b[2], b[3], b[4], b[5],
        );
        if self.hitches > 0 || self.drops > 0 {
            let cause = if self.drops == 0 {
                "capture-bound — frames weren't delivered/captured in time (GPU contention or the 16ms poll); the encoder kept up"
            } else {
                "encoder-bound — the encoder couldn't accept frames fast enough (a lower bitrate/resolution may help)"
            };
            log::warn!("Frame-pacing issues detected — likely {cause}");
        }
    }
}

pub fn find_ffmpeg_path() -> Option<PathBuf> {
    // Bundled extraction dir (now has ffmpeg.exe + all DLLs)
    let bundled = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vrc-recorder")
        .join("ffmpeg.exe");

    if bundled.exists() {
        return Some(bundled);
    }

    // Fallback: VDS path
    let vds = PathBuf::from("C:/Program Files/Virtual Desktop Streamer/ffmpeg.exe");
    if vds.exists() {
        return Some(vds);
    }

    None
}

fn find_ffmpeg() -> Result<PathBuf, String> {
    #[allow(non_snake_case)]
    let CREATE_NO_WINDOW: u32 = 0x08000000;

    if let Some(path) = find_ffmpeg_path() {
        let mut cmd = std::process::Command::new(&path);
        cmd.arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        if cmd.status().is_ok() {
            return Ok(path);
        }
    }

    Err("ffmpeg not found. Install ffmpeg or Virtual Desktop Streamer.".into())
}

/// Compute the timestamp-scale factor and re-timed duration for the merge.
///
/// Video is encoded at a fixed 60fps, so its encoded duration is
/// `frame_count/60`. The real capture rate is rarely exactly 60fps, so we scale
/// the video timestamps by `60 / measured_fps` at merge time (`-itsscale`,
/// copy-only — no re-encode) so the file duration matches wall-clock and audio
/// stays in sync. Returns `(itsscale, target_duration_secs)`.
fn retime_factor(frame_count: u64, wall_elapsed_secs: Option<f64>) -> (f64, f64) {
    // Always 60 — the rawvideo input rate, matching Spout's capture cadence
    let encoded = frame_count as f64 / 60.0;
    match wall_elapsed_secs {
        // Only re-time with a meaningful sample; tiny clips keep the 60fps assumption.
        Some(elapsed) if frame_count > 30 && elapsed > 1.0 => {
            // scale = 60 / measured_fps = elapsed / encoded. Clamp to ~15–240fps
            // so a bogus measurement can't produce an absurd stretch.
            let scale = (elapsed / encoded).clamp(0.25, 4.0);
            (scale, encoded * scale)
        }
        _ => (1.0, encoded),
    }
}

fn merge_audio_video(
    ffmpeg_path: &PathBuf,
    video_path: &PathBuf,
    audio_data: Option<(PathBuf, u32, u16)>,
    mic_data: Option<(PathBuf, u32, u16)>,
    output_path: &PathBuf,
    video_duration_secs: f64,
    itsscale: f64,
) -> Result<(), String> {
    let mut cleanup_paths: Vec<PathBuf> = Vec::new();

    let mut cmd = std::process::Command::new(ffmpeg_path);
    cmd.arg("-y");
    // Scale the video input timestamps to the measured capture rate (P1).
    if (itsscale - 1.0).abs() > 1e-6 {
        cmd.arg("-itsscale").arg(format!("{itsscale:.6}"));
    }
    cmd.arg("-i")
        .arg(video_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .creation_flags(0x08000000_u32);

    let has_sys = audio_data.is_some();
    let has_mic = mic_data.is_some();

    // Add system audio input if present (PCM already streamed to disk).
    if let Some((path, rate, ch)) = audio_data {
        cleanup_paths.push(path.clone());
        cmd.arg("-f").arg("s16le")
            .arg("-ar").arg(rate.to_string())
            .arg("-ac").arg(ch.to_string())
            .arg("-i").arg(&path);
    }

    // Add mic audio input if present (PCM already streamed to disk).
    if let Some((path, rate, ch)) = mic_data {
        cleanup_paths.push(path.clone());
        cmd.arg("-f").arg("s16le")
            .arg("-ar").arg(rate.to_string())
            .arg("-ac").arg(ch.to_string())
            .arg("-i").arg(&path);
    }

    match (has_sys, has_mic) {
        (true, true) => {
            // Mix system + mic; duration=first uses the first (system) audio as length anchor
            cmd.args(["-filter_complex", "[1:a][2:a]amix=inputs=2:duration=first:weights=1 0.5[out]"])
                .args(["-map", "0:v"])
                .args(["-map", "[out]"]);
        }
        (true, false) => {
            cmd.args(["-map", "0:v"]).args(["-map", "1:a"]);
        }
        (false, true) => {
            cmd.args(["-map", "0:v"]).args(["-map", "1:a"]);
        }
        (false, false) => {
            return Err("No audio to merge".into());
        }
    }

    // Use video frame count to set exact output duration. This ensures the
    // video track is never extended (no freeze frames) when audio happens to
    // be longer (e.g. due to encoder startup buffering causing frame drops).
    // When audio is shorter, it ends with silence instead — imperceptible.
    cmd.args(["-t", &format!("{video_duration_secs:.3}")])
        .args(["-c:v", "copy", "-c:a", "aac", "-b:a", "192k"])
        .arg(output_path);

    let output = cmd.output()
        .map_err(|e| format!("Failed to spawn ffmpeg merge: {e}"))?;

    // Cleanup temp files
    for p in &cleanup_paths {
        let _ = std::fs::remove_file(p);
    }

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("ffmpeg merge failed: {stderr}"))
    }
}
