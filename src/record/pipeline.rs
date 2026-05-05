use std::path::PathBuf;

use crate::record::audio::AudioCapture;
use crate::record::capture::PboReader;
use crate::record::encoder::FfmpegEncoder;

pub struct RecordingPipeline {
    encoder: Option<FfmpegEncoder>,
    pbo_reader: Option<PboReader>,
    audio_capture: Option<AudioCapture>,
    output_dir: PathBuf,
    frame_count: u64,
    temp_video_path: Option<PathBuf>,
}

impl RecordingPipeline {
    pub fn new(output_dir: PathBuf) -> Self {
        Self {
            encoder: None,
            pbo_reader: None,
            audio_capture: None,
            output_dir,
            frame_count: 0,
            temp_video_path: None,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.encoder.is_some()
    }

    pub fn start(
        &mut self,
        gl: &glow::Context,
        width: u32,
        height: u32,
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
        )?;

        let pbo = PboReader::new(gl, width, height);

        // Start audio capture
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

        self.encoder = Some(encoder);
        self.pbo_reader = Some(pbo);
        self.audio_capture = audio;
        self.temp_video_path = Some(temp_video);
        self.frame_count = 0;

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
            // Small sleep to let the audio callback flush
            std::thread::sleep(std::time::Duration::from_millis(50));
            let remaining = audio.drain();
            log::info!("Audio captured: {} bytes", remaining.len());
            Some((remaining, audio.sample_rate(), audio.channels()))
        } else {
            None
        };

        // Finish video encoding
        if let Some(mut encoder) = self.encoder.take() {
            log::info!("Finishing video ({} frames)", self.frame_count);
            encoder.finish()?;
        }

        if let Some(pbo) = self.pbo_reader.take() {
            pbo.destroy(gl);
        }

        // Merge with audio if available
        if let Some((audio_pcm, sample_rate, channels)) = audio_data {
            if let Some(ref video_path) = self.temp_video_path {
                let ffmpeg_path = match find_ffmpeg() {
                    Ok(p) => p,
                    Err(e) => {
                        log::error!("{e}");
                        self.temp_video_path = None;
                        self.audio_capture = None;
                        return Err(e);
                    }
                };
                let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
                let final_path = self
                    .output_dir
                    .join(format!("VRCRecorder_{timestamp}.mp4"));

                log::info!(
                    "Merging audio ({} Hz, {} ch, {} bytes) with video...",
                    sample_rate,
                    channels,
                    audio_pcm.len()
                );

                match merge_audio_video(
                    &ffmpeg_path,
                    video_path,
                    &audio_pcm,
                    sample_rate,
                    channels,
                    &final_path,
                ) {
                    Ok(()) => {
                        let _ = std::fs::remove_file(video_path);
                        log::info!("Recording saved to {}", final_path.display());
                    }
                    Err(e) => {
                        log::error!("Audio merge failed: {e}. Keeping video-only file.");
                        // Rename temp video to final name (without audio)
                        let _ = std::fs::remove_file(&final_path);
                        if std::fs::rename(video_path, &final_path).is_ok() {
                            log::info!("Video-only recording saved to {}", final_path.display());
                        }
                    }
                }
            }
        } else if let Some(ref video_path) = self.temp_video_path {
            // No audio — rename temp video to final name
            let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
            let final_path = self
                .output_dir
                .join(format!("VRCRecorder_{timestamp}.mp4"));
            std::fs::rename(video_path, &final_path)
                .map_err(|e| format!("Failed to rename: {e}"))?;
            log::info!("Recording saved to {}", final_path.display());
        }

        self.frame_count = 0;
        self.audio_capture = None;
        self.temp_video_path = None;
        Ok(())
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
                    // Log audio buffer size every 2 seconds (~60 frames at 30fps)
                    if self.frame_count % 60 == 0 {
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
                    }
                }
                Ok(false) => {
                    log::warn!("Encoder full — frame {} dropped", self.frame_count);
                    // Still count it so we don't spam the audio log
                    self.frame_count += 1;
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

fn find_ffmpeg() -> Result<PathBuf, String> {
    let candidates = [
        PathBuf::from("C:/Program Files/Virtual Desktop Streamer/ffmpeg.exe"),
        PathBuf::from("ffmpeg"),
    ];

    for path in &candidates {
        let mut cmd = std::process::Command::new(path);
        cmd.arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if cmd.status().is_ok() {
            return Ok(path.clone());
        }
    }

    Err("ffmpeg not found. Install ffmpeg and ensure it is on PATH.".into())
}

fn merge_audio_video(
    ffmpeg_path: &PathBuf,
    video_path: &PathBuf,
    audio_pcm: &[u8],
    sample_rate: u32,
    channels: u16,
    output_path: &PathBuf,
) -> Result<(), String> {
    let rate = format!("{sample_rate}");

    // Write audio to a temp file to avoid stdin pipe issues
    let audio_path = output_path.with_extension("pcm");
    std::fs::write(&audio_path, audio_pcm)
        .map_err(|e| format!("Failed to write temp audio: {e}"))?;

    let output = std::process::Command::new(ffmpeg_path)
        .args([
            "-y",
            "-i", &video_path.to_string_lossy(),
            "-f", "s16le",
            "-ar", &rate,
            "-ac", &channels.to_string(),
            "-i",
        ])
        .arg(&audio_path)
        .args([
            "-c:v", "copy",
            "-c:a", "aac",
            "-b:a", "192k",
            "-shortest",
        ])
        .arg(output_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to spawn ffmpeg merge: {e}"))?;

    let _ = std::fs::remove_file(&audio_path);

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("ffmpeg merge failed: {stderr}"))
    }
}
