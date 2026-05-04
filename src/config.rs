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
}

fn default_window_width() -> u32 {
    1280
}
fn default_window_height() -> u32 {
    720
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            last_sender: None,
            output_dir: dirs::video_dir().unwrap_or_else(|| PathBuf::from(".")),
            window_width: 1280,
            window_height: 720,
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
