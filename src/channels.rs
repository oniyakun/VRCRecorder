use serde::Serialize;

/// Commands sent from Tauri frontend to the render thread.
#[derive(Debug)]
pub enum RenderCommand {
    ConnectSender(String),
    DisconnectSender,
    StartRecording,
    StopRecording,
    UpdateConfig {
        bitrate: String,
        mirror: bool,
        portrait: bool,
        vflip: bool,
        preview_enabled: bool,
    },
    UpdateOrientation {
        is_landscape: bool,
    },
    Shutdown,
}

/// Events sent from the render thread to Tauri (→ frontend).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum RenderEvent {
    SenderListUpdated {
        senders: Vec<String>,
    },
    SenderConnected {
        name: String,
        width: u32,
        height: u32,
    },
    SenderDisconnected,
    RecordingStarted,
    RecordingStopped {
        final_path: Option<String>,
    },
    RecordingElapsed {
        elapsed_secs: u64,
    },
    PreviewFrame {
        jpeg_base64: String,
        width: u32,
        height: u32,
    },
    ConfigChanged {
        mirror_video: bool,
        portrait_mode: bool,
        video_vflip: bool,
        preview_enabled: bool,
    },
    Error {
        message: String,
    },
    ResolutionChanged {
        old_w: u32,
        old_h: u32,
        new_w: u32,
        new_h: u32,
    },
}
