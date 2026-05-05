use std::io::{BufRead, BufReader, Write};
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
    ) -> Result<Self, String> {
        let size = format!("{width}x{height}");
        let rate = format!("{fps}");

        let mut child = Command::new(ffmpeg_path)
            .args([
                "-y",
                "-f", "rawvideo",
                "-vcodec", "rawvideo",
                "-s", &size,
                "-pix_fmt", "rgba",
                "-r", &rate,
                "-i", "-",
                "-c:v", "libopenh264",
                "-pix_fmt", "yuv420p",
                "-allow_skip_frames", "1",
            ])
            .arg(output_path)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
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

        // Bounded channel: capacity 2 frames (~66MB at 4K)
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(2);

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
