use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub last_sender: Option<String>,
    pub output_dir: PathBuf,
    #[serde(default = "default_window_width")]
    pub window_width: u32,
    #[serde(default = "default_window_height")]
    pub window_height: u32,
    // OSC settings
    #[serde(default = "default_osc_enabled")]
    pub osc_enabled: bool,
    #[serde(default = "default_osc_port")]
    pub osc_port: String,
    #[serde(default = "default_auto_camera")]
    pub auto_camera: bool,
    #[serde(default = "default_auto_stop_camera")]
    pub auto_stop_camera: bool,
    #[serde(default = "default_vrchat_port")]
    pub vrchat_port: String,
    // Video settings
    #[serde(default = "default_video_bitrate")]
    pub video_bitrate: String,
    #[serde(default = "default_mirror_video")]
    pub mirror_video: bool,
    #[serde(default = "default_portrait_mode")]
    pub portrait_mode: bool,
    #[serde(default = "default_video_vflip")]
    pub video_vflip: bool,
    #[serde(default = "default_preview_enabled")]
    pub preview_enabled: bool,
    #[serde(default = "default_mic_enabled")]
    pub mic_enabled: bool,
    #[serde(default)]
    pub mic_device: Option<String>,
    #[serde(default = "default_steamvr_overlay")]
    pub steamvr_overlay: bool,
}

fn default_video_bitrate() -> String {
    "10M".into()
}
fn default_mirror_video() -> bool {
    false
}
fn default_portrait_mode() -> bool {
    false
}
fn default_video_vflip() -> bool {
    false
}
fn default_preview_enabled() -> bool {
    false
}
fn default_mic_enabled() -> bool {
    true
}
fn default_steamvr_overlay() -> bool {
    true
}

fn default_window_width() -> u32 {
    1280
}
fn default_window_height() -> u32 {
    720
}
fn default_osc_enabled() -> bool {
    true
}
fn default_osc_port() -> String {
    "9001".into()
}
fn default_auto_camera() -> bool {
    true
}
fn default_auto_stop_camera() -> bool {
    true
}
fn default_vrchat_port() -> String {
    "9000".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            last_sender: None,
            output_dir: dirs::video_dir().unwrap_or_else(|| PathBuf::from(".")),
            window_width: 1280,
            window_height: 720,
            osc_enabled: true,
            osc_port: "9001".into(),
            auto_camera: true,
            auto_stop_camera: true,
            vrchat_port: "9000".into(),
            video_bitrate: "10M".into(),
            mirror_video: false,
            portrait_mode: false,
            video_vflip: false,
            preview_enabled: false,
            mic_enabled: true,
            mic_device: None,
            steamvr_overlay: true,
        }
    }
}

impl AppConfig {
    fn config_path() -> Option<PathBuf> {
        let dir = dirs::config_dir()?.join("vrc-recorder");
        Some(dir.join("config.json"))
    }

    pub fn load() -> Self {
        let path = match Self::config_path() {
            Some(p) => p,
            None => return Self::default(),
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = match Self::config_path() {
            Some(p) => p,
            None => return,
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }
}
