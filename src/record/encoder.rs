use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::mpsc::{self, TrySendError};
use std::thread;

/// Detected video encoder info.
struct EncoderInfo {
    codec: &'static str,
    /// Extra args added before `-b:v` (preset, quality, etc.)
    extra_args: Vec<&'static str>,
}

/// Detect best available H.264 encoder. Called once, cached forever.
static VIDEO_ENCODER: OnceLock<Result<EncoderInfo, String>> = OnceLock::new();

fn detect_video_encoder(ffmpeg_path: &PathBuf) -> Result<EncoderInfo, String> {
    #[allow(non_snake_case)]
    let CREATE_NO_WINDOW: u32 = 0x08000000;

    let output = Command::new(ffmpeg_path)
        .args(["-encoders"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to query ffmpeg encoders: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Priority: NVENC > AMF > QSV > libx264 > libopenh264
    // All hardware encoders get low-latency options to prevent multi-second
    // encoder-internal buffering from causing duplicate tail frames.
    if stdout.contains("h264_nvenc") {
        log::info!("Video encoder: h264_nvenc (NVIDIA NVENC)");
        return Ok(EncoderInfo {
            codec: "h264_nvenc",
            extra_args: vec!["-preset", "p1", "-rc-lookahead", "0", "-delay", "0"],
        });
    }
    if stdout.contains("h264_amf") {
        log::info!("Video encoder: h264_amf (AMD AMF)");
        return Ok(EncoderInfo {
            codec: "h264_amf",
            extra_args: vec!["-quality", "speed", "-usage", "ultralowlatency"],
        });
    }
    if stdout.contains("h264_qsv") {
        log::info!("Video encoder: h264_qsv (Intel QSV)");
        return Ok(EncoderInfo {
            codec: "h264_qsv",
            extra_args: vec!["-preset", "veryfast", "-look_ahead", "0"],
        });
    }
    if stdout.contains("libx264") {
        log::info!("Video encoder: libx264 (software)");
        return Ok(EncoderInfo {
            codec: "libx264",
            extra_args: vec!["-preset", "ultrafast", "-tune", "zerolatency"],
        });
    }
    // Last resort
    log::info!("Video encoder: libopenh264 (software, slow)");
    Ok(EncoderInfo {
        codec: "libopenh264",
        extra_args: vec![],
    })
}

fn get_video_encoder(ffmpeg_path: &PathBuf) -> Result<&EncoderInfo, &String> {
    VIDEO_ENCODER
        .get_or_init(|| detect_video_encoder(ffmpeg_path))
        .as_ref()
}

/// Encodes RGBA frames to H.264 MP4 via ffmpeg CLI pipe.
/// Frame writes happen on a background thread to avoid blocking the main thread.
pub struct FfmpegEncoder {
    tx: Option<mpsc::SyncSender<Vec<u8>>>,
    writer_thread: Option<thread::JoinHandle<std::io::Result<()>>>,
    width: u32,
    height: u32,
}

impl FfmpegEncoder {
    pub fn start(
        ffmpeg_path: &PathBuf,
        output_path: &PathBuf,
        width: u32,
        height: u32,
        fps: f64,
        bitrate: &str,
        mirror: bool,
        portrait: bool,
        vflip: bool,
    ) -> Result<Self, String> {
        let size = format!("{width}x{height}");
        // Input is always captured at ~60 fps (Spout real-time rate).
        // The user's fps setting controls the OUTPUT frame rate.
        let input_rate = "60".to_string();
        let output_rate = format!("{fps}");

        let enc = get_video_encoder(ffmpeg_path).map_err(|e| e.clone())?;

        // Build video filter: portrait rotation first, then mirror, then vflip
        let mut filters: Vec<String> = Vec::new();
        if portrait {
            filters.push("transpose=2".into());
        }
        if mirror {
            filters.push("hflip".into());
        }
        if vflip {
            filters.push("vflip".into());
        }
        // Hardware encoders need NV12; software needs YUV420p
        let pix_fmt = if enc.codec.contains("nvenc") || enc.codec.contains("qsv") {
            "nv12"
        } else {
            "yuv420p"
        };
        filters.push(format!("format={pix_fmt}"));
        let vf = filters.join(",");

        let mut args: Vec<String> = vec![
            "-y".into(),
            "-f".into(),
            "rawvideo".into(),
            "-vcodec".into(),
            "rawvideo".into(),
            "-s".into(),
            size.clone(),
            "-pix_fmt".into(),
            "rgba".into(),
            "-r".into(),
            input_rate,
            "-i".into(),
            "-".into(),
            "-vf".into(),
            vf,
            "-c:v".into(),
            enc.codec.into(),
        ];
        for a in &enc.extra_args {
            args.push((*a).into());
        }
        args.push("-b:v".into());
        args.push(bitrate.into());
        // Output frame rate (user's choice, 30 or 60)
        args.push("-r".into());
        args.push(output_rate);

        log::info!(
            "ffmpeg: {} {} {}",
            ffmpeg_path.display(),
            args.join(" "),
            output_path.display()
        );

        #[allow(non_snake_case)]
        let CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut child = Command::new(ffmpeg_path)
            .args(&args)
            .arg(output_path)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed to spawn ffmpeg: {e}"))?;

        let mut stdin = child.stdin.take().ok_or("ffmpeg stdin not available")?;

        // Stderr reader thread
        let stderr = child.stderr.take();
        let stderr_thread = stderr.map(|stderr| {
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        log::info!("ffmpeg: {line}");
                    }
                }
            })
        });

        // Bounded channel: capacity 60 frames (~1s at 60fps, ~500MB at 1080p).
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(60);

        // Background writer thread: reads from channel, writes to ffmpeg stdin
        let writer_thread = thread::Builder::new()
            .name("ffmpeg-writer".into())
            .spawn(move || {
                for data in rx {
                    if stdin.write_all(&data).is_err() {
                        break;
                    }
                }
                // Close stdin to signal EOF to ffmpeg
                drop(stdin);
                let status = child.wait()?;
                if !status.success() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("ffmpeg exited with: {status}"),
                    ));
                }
                Ok(())
            })
            .map_err(|e| format!("Failed to spawn writer thread: {e}"))?;

        // Keep stderr thread alive
        if let Some(t) = stderr_thread {
            std::mem::forget(t);
        }

        Ok(Self {
            tx: Some(tx),
            writer_thread: Some(writer_thread),
            width,
            height,
        })
    }

    /// Try to enqueue a frame. If the encoder is backed up, the frame is dropped
    /// and `false` is returned. Never blocks.
    pub fn push_frame(&mut self, rgba_data: Vec<u8>) -> Result<bool, String> {
        let expected = (self.width * self.height * 4) as usize;
        if rgba_data.len() != expected {
            return Err(format!(
                "Frame size mismatch: got {}, expected {expected}",
                rgba_data.len()
            ));
        }

        let tx = self.tx.as_ref().ok_or("Encoder not initialized")?;
        match tx.try_send(rgba_data) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => Ok(false),
            Err(TrySendError::Disconnected(_)) => Err("Encoder writer thread disconnected".into()),
        }
    }

    /// Blocking send — waits for channel space. Only used during final flush
    /// to guarantee the last frames are never dropped.
    pub fn push_frame_blocking(&mut self, rgba_data: Vec<u8>) -> Result<(), String> {
        let expected = (self.width * self.height * 4) as usize;
        if rgba_data.len() != expected {
            return Err(format!(
                "Frame size mismatch: got {}, expected {expected}",
                rgba_data.len()
            ));
        }

        let tx = self.tx.as_ref().ok_or("Encoder not initialized")?;
        tx.send(rgba_data)
            .map_err(|_| "Encoder writer thread disconnected".into())
    }

    pub fn finish(&mut self) -> Result<(), String> {
        // Close channel to signal writer thread to finish
        drop(self.tx.take());

        if let Some(thread) = self.writer_thread.take() {
            match thread.join() {
                Ok(Ok(())) => {
                    log::info!("ffmpeg encoder finished successfully");
                    Ok(())
                }
                Ok(Err(e)) => Err(format!("ffmpeg encoder error: {e}")),
                Err(_) => Err("ffmpeg writer thread panicked".into()),
            }
        } else {
            Ok(())
        }
    }
}

impl Drop for FfmpegEncoder {
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(thread) = self.writer_thread.take() {
            let _ = thread.join();
        }
    }
}
