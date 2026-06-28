use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Manages WASAPI loopback capture in a background thread.
///
/// PCM is streamed straight to a temp `.pcm` file via a dedicated writer thread
/// instead of being accumulated in RAM, so long recordings don't grow unbounded
/// and the realtime audio callback never reallocates a large buffer (see P2).
pub struct AudioCapture {
    stream: Option<cpal::Stream>,
    writer_thread: Option<JoinHandle<u64>>,
    sample_rate: u32,
    channels: u16,
    pcm_path: PathBuf,
}

impl AudioCapture {
    /// Start loopback capture, streaming s16le PCM into `pcm_path`.
    pub fn start(pcm_path: PathBuf) -> Result<Self, String> {
        let host = cpal::default_host();

        let device = host
            .default_output_device()
            .ok_or("No output audio device found")?;

        let out_config = device
            .default_output_config()
            .map_err(|e| format!("Failed to get output config: {e}"))?;

        let sample_rate = out_config.sample_rate();
        let channels = out_config.channels();

        // Build an input stream on the output device for loopback capture
        let stream_config = cpal::StreamConfig {
            channels,
            sample_rate: out_config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        log::info!(
            "Audio capture: {} Hz, {} channels → {}",
            sample_rate,
            channels,
            pcm_path.display()
        );

        let file = File::create(&pcm_path)
            .map_err(|e| format!("Failed to create audio temp file: {e}"))?;

        // Writer thread drains the channel to disk. The realtime callback only
        // sends (never blocks, never reallocates a growing buffer).
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let writer_thread = std::thread::Builder::new()
            .name("audio-writer".into())
            .spawn(move || {
                let mut writer = BufWriter::new(file);
                let mut total: u64 = 0;
                for chunk in rx {
                    if writer.write_all(&chunk).is_err() {
                        break;
                    }
                    total += chunk.len() as u64;
                }
                let _ = writer.flush();
                total
            })
            .map_err(|e| format!("Failed to spawn audio writer: {e}"))?;

        let stream = device
            .build_input_stream::<f32, _, _>(
                stream_config,
                move |data: &[f32], _info| {
                    // Convert f32 samples to s16le PCM interleaved (fixed-size
                    // per-callback Vec — no growing shared buffer, no lock).
                    let mut pcm = Vec::with_capacity(data.len() * 2);
                    for &s in data {
                        let clamped = s.clamp(-1.0, 1.0);
                        let sample = (clamped * 32767.0) as i16;
                        pcm.extend_from_slice(&sample.to_le_bytes());
                    }
                    let _ = tx.send(pcm);
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
            writer_thread: Some(writer_thread),
            sample_rate,
            channels,
            pcm_path,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Path of the temp PCM file this capture is writing to.
    pub fn pcm_path(&self) -> &PathBuf {
        &self.pcm_path
    }

    /// Stop the stream and finalize the temp file. Returns total bytes written.
    pub fn stop(&mut self) -> u64 {
        // Drop the stream first (stops callbacks and closes the channel sender
        // held by the callback), then join the writer so the file is flushed.
        self.stream = None;
        if let Some(t) = self.writer_thread.take() {
            t.join().unwrap_or(0)
        } else {
            0
        }
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        if self.writer_thread.is_some() {
            let _ = self.stop();
        }
    }
}
