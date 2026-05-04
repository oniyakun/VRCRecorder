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
    #[serde(default = "default_osc_record_address")]
    pub osc_record_address: String,
    #[serde(default = "default_auto_camera")]
    pub auto_camera: bool,
    #[serde(default = "default_vrchat_port")]
    pub vrchat_port: String,
}

fn default_window_width() -> u32 {
    1280
}
fn default_window_height() -> u32 {
    720
}
fn default_osc_enabled() -> bool {
    false
}
fn default_osc_port() -> String {
    "9000".into()
}
fn default_osc_record_address() -> String {
    "/avatar/parameters/VRCRecord".into()
}
fn default_auto_camera() -> bool {
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
            osc_enabled: false,
            osc_port: "9000".into(),
            osc_record_address: "/avatar/parameters/VRCRecord".into(),
            auto_camera: true,
            vrchat_port: "9000".into(),
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
