use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, TrySendError};
use std::thread;

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
        let rate = format!("{fps}");

        // Build video filter: portrait rotation first, then mirror, then vflip
        // (so mirror flips left-right in portrait space, vflip fixes back camera)
        let mut filters: Vec<&str> = Vec::new();
        if portrait {
            filters.push("transpose=2");
        }
        if mirror {
            filters.push("hflip");
        }
        if vflip {
            filters.push("vflip");
        }
        let vf = filters.join(",");

        let mut args: Vec<String> = vec![
            "-y".into(),
            "-f".into(), "rawvideo".into(),
            "-vcodec".into(), "rawvideo".into(),
            "-s".into(), size.clone(),
            "-pix_fmt".into(), "rgba".into(),
            "-r".into(), rate.clone(),
            "-i".into(), "-".into(),
            "-c:v".into(), "libopenh264".into(),
            "-b:v".into(), bitrate.into(),
            "-pix_fmt".into(), "yuv420p".into(),
            "-allow_skip_frames".into(), "1".into(),
        ];
        if !vf.is_empty() {
            args.push("-vf".into());
            args.push(vf);
        }

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

        let mut stdin = child
            .stdin
            .take()
            .ok_or("ffmpeg stdin not available")?;

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

        // Bounded channel: capacity 16 frames (~530MB at 4K)
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(16);

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
            // Leak it intentionally — it dies when ffmpeg process exits
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

        let tx = self
            .tx
            .as_ref()
            .ok_or("Encoder not initialized")?;
        match tx.try_send(rgba_data) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => Ok(false),
            Err(TrySendError::Disconnected(_)) => Err("Encoder writer thread disconnected".into()),
        }
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
