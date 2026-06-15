use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Manages microphone capture in a background thread.
pub struct MicCapture {
    stream: Option<cpal::Stream>,
    buffer: Arc<Mutex<Vec<u8>>>,
    sample_rate: u32,
    channels: u16,
    device_name: String,
}

impl MicCapture {
    /// Start capturing from the specified microphone device.
    /// If `device_name` is None, uses the system default input device.
    pub fn start(device_name: Option<&str>) -> Result<Self, String> {
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
        log::info!("Mic capture: device='{actual_name}'");

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

        let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let buf_clone = buffer.clone();

        let stream = device
            .build_input_stream::<f32, _, _>(
                stream_config,
                move |data: &[f32], _info| {
                    // Convert f32 samples to s16le PCM
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
                    log::error!("Mic capture error: {err}");
                },
                None,
            )
            .map_err(|e| format!("Failed to start mic stream: {e}"))?;

        stream.play().map_err(|e| format!("Failed to play mic stream: {e}"))?;

        Ok(Self {
            stream: Some(stream),
            buffer,
            sample_rate,
            channels,
            device_name: actual_name,
        })
    }

    /// Check if the default input device has changed.
    pub fn device_changed(&self) -> bool {
        let host = cpal::default_host();
        if let Some(default) = host.default_input_device() {
            if let Ok(desc) = default.description() {
                return desc.name() != self.device_name;
            }
        }
        false
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
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
