use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Manages WASAPI loopback capture in a background thread.
pub struct AudioCapture {
    stream: Option<cpal::Stream>,
    buffer: Arc<Mutex<Vec<u8>>>,
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

        let sample_rate = out_config.sample_rate();
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

        let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let buf_clone = buffer.clone();

        let stream = device
            .build_input_stream::<f32, _, _>(
                stream_config,
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

                    if let Ok(mut buf) = buf_clone.lock() {
                        buf.extend_from_slice(&pcm);
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
            buffer,
            sample_rate,
            channels,
        })
    }

    /// Drain all captured audio data since last call.
    pub fn drain(&mut self) -> Vec<u8> {
        if let Ok(mut buf) = self.buffer.lock() {
            std::mem::take(&mut *buf)
        } else {
            Vec::new()
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Current size of the captured audio buffer in bytes (without draining).
    pub fn buffer_size(&self) -> usize {
        if let Ok(buf) = self.buffer.lock() {
            buf.len()
        } else {
            0
        }
    }

    pub fn stop(&mut self) {
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }
    }
}
