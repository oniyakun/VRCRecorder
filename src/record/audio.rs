use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Manages WASAPI loopback capture in a background thread.
/// PCM data is written to a temporary file instead of an in-memory buffer,
/// avoiding unbounded memory growth during long recordings.
pub struct AudioCapture {
    stream: Option<cpal::Stream>,
    writer: Arc<Mutex<Option<BufWriter<File>>>>,
    file_path: PathBuf,
    sample_rate: u32,
    channels: u16,
}

impl AudioCapture {
    pub fn start() -> Result<Self, String> {
        let host = cpal::default_host();

        let device = host
            .default_output_device()
            .ok_or("No output audio device found")?;

        let out_config = device
            .default_output_config()
            .map_err(|e| format!("Failed to get output config: {e}"))?;

        let sample_rate = out_config.sample_rate().0;
        let channels = out_config.channels();

        // Build an input stream on the output device for loopback capture
        let stream_config = cpal::StreamConfig {
            channels,
            sample_rate: out_config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        log::info!(
            "Audio capture: {} Hz, {} channels",
            sample_rate,
            channels
        );

        // Create temp file for PCM data — avoids unbounded memory growth
        let file_path = std::env::temp_dir()
            .join(format!("vrc_sys_audio_{}.pcm", std::process::id()));
        let file = File::create(&file_path)
            .map_err(|e| format!("Failed to create audio temp file: {e}"))?;
        let writer = Arc::new(Mutex::new(Some(BufWriter::with_capacity(256 * 1024, file))));
        let writer_clone = writer.clone();

        let stream = device
            .build_input_stream::<f32, _, _>(
                &stream_config,
                move |data: &[f32], _info| {
                    // Convert f32 samples to s16le PCM interleaved
                    let pcm: Vec<u8> = data
                        .iter()
                        .flat_map(|&s| {
                            let clamped = s.clamp(-1.0, 1.0);
                            let sample = (clamped * 32767.0) as i16;
                            sample.to_le_bytes()
                        })
                        .collect();

                    if let Ok(mut guard) = writer_clone.lock() {
                        if let Some(ref mut w) = *guard {
                            let _ = w.write_all(&pcm);
                        }
                    }
                },
                |err| {
                    log::error!("Audio capture error: {err}");
                },
                None,
            )
            .map_err(|e| format!("Failed to start audio capture: {e}"))?;

        stream.play().map_err(|e| format!("Failed to play: {e}"))?;

        Ok(Self {
            stream: Some(stream),
            writer,
            file_path,
            sample_rate,
            channels,
        })
    }

    /// Finish capture: flush and close the temp file, returning its path,
    /// sample rate, and channel count. The caller is responsible for
    /// deleting the file when done.
    pub fn finish(&mut self) -> Option<(PathBuf, u32, u16)> {
        if let Ok(mut guard) = self.writer.lock() {
            if let Some(writer) = guard.take() {
                drop(writer); // flush and close
                return Some((self.file_path.clone(), self.sample_rate, self.channels));
            }
        }
        None
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Current size of the captured audio file in bytes.
    pub fn buffer_size(&self) -> u64 {
        std::fs::metadata(&self.file_path)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    pub fn stop(&mut self) {
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }
        // Note: we do NOT delete the temp file here — finish() handles that.
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        // Clean up temp file if finish() was never called
        let _ = std::fs::remove_file(&self.file_path);
    }
}
