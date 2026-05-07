use std::path::PathBuf;
use std::os::windows::process::CommandExt;

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
    dropped_count: u64,
    temp_video_path: Option<PathBuf>,
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
            dropped_count: 0,
            temp_video_path: None,
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
        let fps = 60.0;

        std::fs::create_dir_all(&self.output_dir)
            .map_err(|e| format!("Failed to create output dir: {e}"))?;

        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");

        // Record video-only first to a temp file
        let temp_video = self.output_dir.join(format!(
            "VRCRecorder_{timestamp}_video.mp4"
        ));

        let encoder = FfmpegEncoder::start(
            &ffmpeg_path,
            &temp_video,
            width,
            height,
            fps,
            bitrate,
            mirror,
            portrait,
            vflip,
        )?;

        let pbo = PboReader::new(gl, width, height);

        // Start system audio capture
        let audio = match AudioCapture::start() {
            Ok(a) => {
                log::info!("System audio capture started");
                Some(a)
            }
            Err(e) => {
                log::warn!("Audio capture unavailable: {e}. Recording video only.");
                None
            }
        };

        // Start mic capture if enabled
        let mic = if mic_enabled {
            match MicCapture::start(mic_device) {
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
        self.temp_video_path = Some(temp_video);
        self.frame_count = 0;
        self.dropped_count = 0;

        log::info!(
            "Recording started: {}x{}",
            width,
            height,
        );
        Ok(())
    }

    pub fn stop(&mut self, gl: &glow::Context) -> Result<(), String> {
        // Stop audio capture first, then drain remaining data
        let audio_data = if let Some(ref mut audio) = self.audio_capture {
            audio.stop();
            std::thread::sleep(std::time::Duration::from_millis(50));
            let remaining = audio.drain();
            log::info!("System audio captured: {} bytes", remaining.len());
            Some((remaining, audio.sample_rate(), audio.channels()))
        } else {
            None
        };

        // Stop and drain mic capture
        let mic_data = if let Some(ref mut mic) = self.mic_capture {
            mic.stop();
            std::thread::sleep(std::time::Duration::from_millis(50));
            let remaining = mic.drain();
            log::info!("Mic audio captured: {} bytes", remaining.len());
            if remaining.is_empty() {
                None
            } else {
                Some((remaining, mic.sample_rate(), mic.channels()))
            }
        } else {
            None
        };

        // Flush PBO pipeline: drain remaining frames before finishing encoder
        if let Some(ref mut pbo) = self.pbo_reader {
            for data in pbo.flush(gl) {
                if let Some(ref mut encoder) = self.encoder {
                    match encoder.push_frame(data) {
                        Ok(true) => self.frame_count += 1,
                        Ok(false) => log::warn!("Encoder full during PBO flush"),
                        Err(e) => log::error!("Encoder error during flush: {e}"),
                    }
                }
            }
        }

        // Take ownership of encoder, video path, output dir for background finalization.
        // finish() and audio merge block for seconds — must NOT run on the render thread
        // or we lose Spout frames arriving during finalization.
        let encoder = self.encoder.take();
        let video_path = self.temp_video_path.take();
        let frame_count = self.frame_count;
        let dropped_count = self.dropped_count;
        let output_dir = self.output_dir.clone();

        // Clean up GPU resources on render thread
        if let Some(pbo) = self.pbo_reader.take() {
            pbo.destroy(gl);
        }

        // Reset state immediately so a new recording can start
        self.frame_count = 0;
        self.dropped_count = 0;
        self.audio_capture = None;
        self.temp_video_path = None;

        // Finalize encoding and merge audio on a background thread
        if encoder.is_some() || video_path.is_some() {
            std::thread::Builder::new()
                .name("encoder-finish".into())
                .spawn(move || {
                    // Finish video encoding
                    if let Some(mut enc) = encoder {
                        log::info!(
                            "Finishing video: {} frames queued, {} dropped ({:.1}%)",
                            frame_count,
                            dropped_count,
                            if frame_count + dropped_count > 0 {
                                (dropped_count as f64 / (frame_count + dropped_count) as f64) * 100.0
                            } else {
                                0.0
                            }
                        );
                        if let Err(e) = enc.finish() {
                            log::error!("Encoder finish failed: {e}");
                            // Video file may be corrupt; keep it for debugging
                            if let Some(ref vp) = video_path {
                                log::info!("Partial video kept at {}", vp.display());
                            }
                            return;
                        }
                    }

                    // Merge audio using ffmpeg
                    if let Some(ref vp) = video_path {
                        let ffmpeg_path = match find_ffmpeg() {
                            Ok(p) => p,
                            Err(e) => {
                                log::error!("{e}");
                                return;
                            }
                        };
                        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
                        let final_path = output_dir.join(format!("VRCRecorder_{timestamp}.mp4"));

                        match merge_audio_video(
                            &ffmpeg_path,
                            vp,
                            audio_data,
                            mic_data,
                            &final_path,
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

        Ok(())
    }

    /// Check if the default mic device changed and restart capture if needed.
    /// Should be called periodically (e.g., every 2s) during recording.
    pub fn check_mic_device(&mut self) {
        if let Some(ref mic) = self.mic_capture {
            if mic.device_changed() {
                log::info!("Mic device changed, restarting capture...");
                let _ = self.mic_capture.take();
                match MicCapture::start(None) {
                    Ok(m) => {
                        log::info!("Mic capture restarted: {}", m.device_name());
                        self.mic_capture = Some(m);
                    }
                    Err(e) => log::warn!("Mic restart failed: {e}"),
                }
            }
        }
    }

    /// Call each frame: PBO readback → non-blocking encoder push.
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
                    // Log audio buffer size every 2s (~120 frames at 60fps)
                    if self.frame_count % 120 == 0 {
                        if let Some(ref audio) = self.audio_capture {
                            let buf_size = audio.buffer_size();
                            if buf_size > 0 {
                                log::info!(
                                    "Audio buffer: {} bytes (~{:.1}s)",
                                    buf_size,
                                    buf_size as f64
                                        / (audio.sample_rate() as f64
                                            * audio.channels() as f64
                                            * 2.0)
                                );
                            }
                        }
                        if self.dropped_count > 0 {
                            log::warn!(
                                "Encoder dropped {} frames so far ({} queued)",
                                self.dropped_count,
                                self.frame_count
                            );
                        }
                    }
                }
                Ok(false) => {
                    self.dropped_count += 1;
                    if self.dropped_count % 60 == 1 {
                        log::warn!(
                            "Encoder full — {} frames dropped so far ({} queued)",
                            self.dropped_count,
                            self.frame_count
                        );
                    }
                }
                Err(e) => {
                    log::error!("Encoder error on frame {}: {e}", self.frame_count);
                }
            }
        }
    }
}

impl Drop for RecordingPipeline {
    fn drop(&mut self) {
        if let Some(mut encoder) = self.encoder.take() {
            let _ = encoder.finish();
        }
        if let Some(ref mut audio) = self.audio_capture {
            audio.stop();
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

fn merge_audio_video(
    ffmpeg_path: &PathBuf,
    video_path: &PathBuf,
    audio_data: Option<(Vec<u8>, u32, u16)>,
    mic_data: Option<(Vec<u8>, u32, u16)>,
    output_path: &PathBuf,
) -> Result<(), String> {
    let audio_path = output_path.with_extension("sys.pcm");
    let mic_path = output_path.with_extension("mic.pcm");
    let mut cleanup_paths: Vec<PathBuf> = Vec::new();

    let mut cmd = std::process::Command::new(ffmpeg_path);
    cmd.arg("-y")
        .arg("-i")
        .arg(video_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .creation_flags(0x08000000_u32);

    let has_sys = audio_data.is_some();
    let has_mic = mic_data.is_some();

    // Add system audio input if present
    if let Some((pcm, rate, ch)) = audio_data {
        std::fs::write(&audio_path, &pcm)
            .map_err(|e| format!("Failed to write system audio: {e}"))?;
        cleanup_paths.push(audio_path.clone());
        cmd.arg("-f").arg("s16le")
            .arg("-ar").arg(rate.to_string())
            .arg("-ac").arg(ch.to_string())
            .arg("-i").arg(&audio_path);
    }

    // Add mic audio input if present
    if let Some((pcm, rate, ch)) = mic_data {
        std::fs::write(&mic_path, &pcm)
            .map_err(|e| format!("Failed to write mic audio: {e}"))?;
        cleanup_paths.push(mic_path.clone());
        cmd.arg("-f").arg("s16le")
            .arg("-ar").arg(rate.to_string())
            .arg("-ac").arg(ch.to_string())
            .arg("-i").arg(&mic_path);
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

    // No -shortest: video is the authoritative length. Audio stops slightly before
    // video (audio.stop() is called before PBO flush), so -shortest would truncate
    // the video to match the shorter audio track.
    cmd.args(["-c:v", "copy", "-c:a", "aac", "-b:a", "192k"])
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
