use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;

/// Encodes RGBA frames to H.264 MP4 via ffmpeg CLI pipe.
pub struct FfmpegEncoder {
    child: Child,
    _stderr_thread: Option<thread::JoinHandle<()>>,
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

        // Spawn a thread to capture ffmpeg stderr for logging
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

        Ok(Self {
            child,
            _stderr_thread: stderr_thread,
            width,
            height,
        })
    }

    pub fn push_frame(&mut self, rgba_data: &[u8]) -> Result<(), String> {
        let expected = (self.width * self.height * 4) as usize;
        if rgba_data.len() != expected {
            return Err(format!(
                "Frame size mismatch: got {}, expected {expected}",
                rgba_data.len()
            ));
        }

        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or("ffmpeg stdin not available")?;

        stdin
            .write_all(rgba_data)
            .map_err(|e| format!("Failed to write frame to ffmpeg: {e}"))?;

        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), String> {
        log::info!("Closing ffmpeg stdin and waiting for encode to finish...");
        drop(self.child.stdin.take());

        let status = self
            .child
            .wait()
            .map_err(|e| format!("Failed to wait for ffmpeg: {e}"))?;

        if status.success() {
            log::info!("ffmpeg finished successfully");
            Ok(())
        } else {
            Err(format!("ffmpeg exited with status: {status}"))
        }
    }
}

impl Drop for FfmpegEncoder {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
    }
}
