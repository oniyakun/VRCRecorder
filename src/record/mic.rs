use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Manages microphone capture in a background thread.
///
/// Like [`AudioCapture`](crate::record::audio::AudioCapture), PCM is streamed to
/// a temp `.pcm` file via a writer thread rather than buffered in RAM (P2).
pub struct MicCapture {
    stream: Option<cpal::Stream>,
    writer_thread: Option<JoinHandle<u64>>,
    sample_rate: u32,
    channels: u16,
    device_name: String,
    pcm_path: PathBuf,
}

impl MicCapture {
    /// Start capturing from the specified microphone device into `pcm_path`.
    /// If `device_name` is None, uses the system default input device.
    pub fn start(device_name: Option<&str>, pcm_path: PathBuf) -> Result<Self, String> {
        let host = cpal::default_host();

        let device = if let Some(name) = device_name {
            // Find the named input device
            host.input_devices()
                .map_err(|e| format!("Failed to enumerate input devices: {e}"))?
                .find(|d| d.description().map(|desc| desc.name() == name).unwrap_or(false))
                .ok_or_else(|| format!("Microphone '{name}' not found"))?
        } else {
            host.default_input_device()
                .ok_or("No input audio device found")?
        };

        let actual_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_default();
        log::info!("Mic capture: device='{actual_name}' → {}", pcm_path.display());

        let config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get input config: {e}"))?;

        let sample_rate = config.sample_rate();
        let channels = config.channels();

        let stream_config = cpal::StreamConfig {
            channels,
            sample_rate: config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        let file = File::create(&pcm_path)
            .map_err(|e| format!("Failed to create mic temp file: {e}"))?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let writer_thread = std::thread::Builder::new()
            .name("mic-writer".into())
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
            .map_err(|e| format!("Failed to spawn mic writer: {e}"))?;

        let stream = device
            .build_input_stream::<f32, _, _>(
                stream_config,
                move |data: &[f32], _info| {
                    // Convert f32 samples to s16le PCM (fixed-size per-callback
                    // Vec — no growing shared buffer, no lock).
                    let mut pcm = Vec::with_capacity(data.len() * 2);
                    for &s in data {
                        let clamped = s.clamp(-1.0, 1.0);
                        let sample = (clamped * 32767.0) as i16;
                        pcm.extend_from_slice(&sample.to_le_bytes());
                    }
                    let _ = tx.send(pcm);
                },
                |err| {
                    log::error!("Mic capture error: {err}");
                },
                None,
            )
            .map_err(|e| format!("Failed to start mic stream: {e}"))?;

        stream.play().map_err(|e| format!("Failed to play mic stream: {e}"))?;

        Ok(Self {
            stream: Some(stream),
            writer_thread: Some(writer_thread),
            sample_rate,
            channels,
            device_name: actual_name,
            pcm_path,
        })
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
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
        self.stream = None;
        if let Some(t) = self.writer_thread.take() {
            t.join().unwrap_or(0)
        } else {
            0
        }
    }
}

impl Drop for MicCapture {
    fn drop(&mut self) {
        if self.writer_thread.is_some() {
            let _ = self.stop();
        }
    }
}

/// List available input device names.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devices) => devices
            .filter_map(|d| d.description().ok().map(|desc| desc.name().to_string()))
            .collect(),
        Err(_) => Vec::new(),
    }
}
