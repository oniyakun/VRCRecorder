#![windows_subsystem = "windows"]

mod bundle;
mod channels;
mod config;
mod mjpeg_server;
mod osc;
mod record;
mod render;
mod render_thread;
mod spout;
mod steamvr;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use tauri::Manager;
use tauri::menu::{MenuBuilder, MenuItem};
use tauri::tray::TrayIconBuilder;

use channels::{RenderCommand, RenderEvent};
use config::AppConfig;
use osc::OscReceiver;

pub struct AppState {
    pub config: Mutex<AppConfig>,
    cmd_tx: Mutex<mpsc::Sender<RenderCommand>>,
    event_rx: Mutex<mpsc::Receiver<RenderEvent>>,
    pub osc_receiver: Mutex<Option<OscReceiver>>,
    pub preview_buf: Arc<Mutex<Vec<u8>>>,
    pub preview_dims: Arc<Mutex<(u32, u32)>>,
    pub mjpeg_port: u16,
    /// Whether to auto-launch SteamVR if not running. Shared live with the
    /// SteamVR overlay thread so toggling it in the UI takes effect immediately.
    pub steamvr_auto_launch: Arc<AtomicBool>,
}

// ===== Tauri Commands =====

#[tauri::command]
fn connect_sender(state: tauri::State<AppState>, name: String) -> Result<(), String> {
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.last_sender = Some(name.clone());
        cfg.save();
    }
    state
        .cmd_tx
        .lock()
        .unwrap()
        .send(RenderCommand::ConnectSender(name))
        .map_err(|e| format!("Failed to send command: {e}"))
}

#[tauri::command]
fn disconnect_sender(state: tauri::State<AppState>) -> Result<(), String> {
    state
        .cmd_tx
        .lock()
        .unwrap()
        .send(RenderCommand::DisconnectSender)
        .map_err(|e| format!("Failed to send command: {e}"))
}

#[tauri::command]
fn start_recording(state: tauri::State<AppState>) -> Result<(), String> {
    state
        .cmd_tx
        .lock()
        .unwrap()
        .send(RenderCommand::StartRecording)
        .map_err(|e| format!("Failed to send command: {e}"))
}

#[tauri::command]
fn stop_recording(state: tauri::State<AppState>) -> Result<(), String> {
    state
        .cmd_tx
        .lock()
        .unwrap()
        .send(RenderCommand::StopRecording)
        .map_err(|e| format!("Failed to send command: {e}"))
}

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> Result<AppConfig, String> {
    let cfg = state.config.lock().unwrap();
    Ok(cfg.clone())
}

#[tauri::command]
fn update_config(state: tauri::State<AppState>, config: AppConfig) -> Result<(), String> {
    let bitrate = config.video_bitrate.clone();
    let mirror = config.mirror_video;
    let portrait = config.portrait_mode;
    let vflip = config.video_vflip;
    let preview_enabled = config.preview_enabled;
    let mic_enabled = config.mic_enabled;
    let mic_device = config.mic_device.clone();
    let auto_stop_camera = config.auto_stop_camera;
    let output_dir = config.output_dir.clone();
    let steamvr_auto_launch = config.steamvr_auto_launch;

    {
        let mut cfg = state.config.lock().unwrap();
        *cfg = config;
        cfg.save();
    }

    // Push the auto-launch preference to the SteamVR overlay thread live.
    state
        .steamvr_auto_launch
        .store(steamvr_auto_launch, Ordering::Relaxed);

    let _ = state.cmd_tx.lock().unwrap().send(RenderCommand::UpdateConfig {
        bitrate,
        mirror,
        portrait,
        vflip,
        preview_enabled,
        mic_enabled,
        mic_device,
        auto_stop_camera,
        output_dir,
    });

    Ok(())
}

#[tauri::command]
fn poll_events(state: tauri::State<AppState>) -> Result<Vec<RenderEvent>, String> {
    let mut events = Vec::new();
    let rx = state.event_rx.lock().unwrap();
    while let Ok(evt) = rx.try_recv() {
        let is_res_change = matches!(evt, RenderEvent::ResolutionChanged { .. });
        events.push(evt);
        if is_res_change {
            let exe = std::env::current_exe().unwrap_or_default();
            let _ = std::process::Command::new(&exe).spawn();
            std::process::exit(0);
        }
    }
    Ok(events)
}

#[tauri::command]
fn get_stream_port(state: tauri::State<AppState>) -> u16 {
    state.mjpeg_port
}

#[tauri::command]
fn select_output_dir(state: tauri::State<AppState>) -> Result<String, String> {
    let start_dir = state.config.lock().unwrap().output_dir.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let path = rfd::FileDialog::new()
            .set_directory(start_dir)
            .pick_folder();
        let _ = tx.send(path);
    });

    match rx.recv() {
        Ok(Some(path)) => Ok(path.to_string_lossy().into_owned()),
        Ok(None) => Err("No folder selected".into()),
        Err(_) => Err("Dialog thread failed".into()),
    }
}

#[tauri::command]
fn list_mic_devices() -> Vec<String> {
    crate::record::mic::list_input_devices()
}

// ===== OSC Management =====

fn sync_osc(state: &AppState) {
    let (enabled, port) = {
        let cfg = state.config.lock().unwrap();
        (cfg.osc_enabled, cfg.osc_port.clone())
    };

    let mut osc = state.osc_receiver.lock().unwrap();
    if let Some(mut receiver) = osc.take() {
        receiver.stop();
    }

    if enabled {
        let cmd_tx = state.cmd_tx.lock().unwrap().clone();
        match OscReceiver::start(
            port.parse().unwrap_or(9001),
            move |msg| {
                if msg.address == "/usercamera/OrientationIsLandscape" {
                    let _ = cmd_tx.send(RenderCommand::UpdateOrientation {
                        is_landscape: match msg.value {
                            osc::OscValue::Int(v) => v != 0,
                            osc::OscValue::Bool(v) => v,
                            osc::OscValue::Float(v) => v > 0.5,
                        },
                    });
                }
            },
        ) {
            Ok(receiver) => {
                log::info!("OSC receiver started on port {port}");
                *osc = Some(receiver);
            }
            Err(e) => {
                log::error!("Failed to start OSC receiver: {e}");
            }
        }
    }
}

// ===== Entry Point =====

fn main() {
    // Write logs to file in cache dir for debugging
    let log_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("vrc-recorder");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("vrc-recorder.log");
    let log_file = std::fs::File::create(&log_path)
        .expect("Failed to create log file");

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format_timestamp_millis()
    .target(env_logger::Target::Pipe(Box::new(log_file)))
    .init();

    // Log panics
    let log_path_clone = log_path.clone();
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&log_path_clone) {
            let _ = writeln!(f, "PANIC: {info}");
        }
    }));

    log::info!("VRCRecorder starting, log at {}", log_path.display());

    let _ = bundle::extract();

    let config = AppConfig::load();

    let (cmd_tx, cmd_rx) = mpsc::channel::<RenderCommand>();
    let (event_tx, event_rx) = mpsc::channel::<RenderEvent>();

    // Recording state flag shared with SteamVR overlay thread
    let is_recording = Arc::new(AtomicBool::new(false));

    // Auto-launch-SteamVR flag, shared live with the overlay thread
    let steamvr_auto_launch = Arc::new(AtomicBool::new(config.steamvr_auto_launch));

    // Shared preview buffer (render thread writes, protocol handler reads)
    let preview_buf = Arc::new(Mutex::new(Vec::new()));
    let preview_dims = Arc::new(Mutex::new((0u32, 0u32)));

    // Clone before moving into render thread / setup closure
    let is_recording_for_overlay = is_recording.clone();
    let auto_launch_for_overlay = steamvr_auto_launch.clone();

    let render_config = config.clone();
    render_thread::spawn(
        render_config,
        cmd_rx,
        event_tx,
        preview_buf.clone(),
        preview_dims.clone(),
        is_recording.clone(),
    );

    // Start MJPEG streaming server
    let mjpeg_port = mjpeg_server::start(mjpeg_server::MjpegState {
        jpeg_data: preview_buf.clone(),
    });

    let app_state = AppState {
        config: Mutex::new(config),
        cmd_tx: Mutex::new(cmd_tx.clone()),
        event_rx: Mutex::new(event_rx),
        osc_receiver: Mutex::new(None),
        preview_buf,
        preview_dims,
        mjpeg_port,
        steamvr_auto_launch,
    };

    tauri::Builder::default()
        .setup(move |app| {
            // --- System tray icon ---
            let logo_rgba = image::load_from_memory(include_bytes!("../logo.png"))
                .expect("Failed to load tray icon")
                .to_rgba8();
            let (icon_w, icon_h) = logo_rgba.dimensions();
            let icon = tauri::image::Image::new_owned(logo_rgba.into_raw(), icon_w, icon_h);

            let show_item = MenuItem::with_id(app, "show", "Show", true, None::<&str>)
                .expect("Failed to create Show menu item");
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
                .expect("Failed to create Quit menu item");
            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&quit_item)
                .build()
                .expect("Failed to build tray menu");

            TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } = event
                    {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)
                .expect("Failed to build tray icon");

            sync_osc(&*app.state::<AppState>());

            // Spawn SteamVR overlay thread (no-op if disabled in config)
            let overlay_config = app.state::<AppState>().config.lock().unwrap().clone();
            let overlay_cmd_tx = app.state::<AppState>().cmd_tx.lock().unwrap().clone();
            steamvr::spawn_overlay(
                overlay_config,
                overlay_cmd_tx,
                is_recording_for_overlay.clone(),
                auto_launch_for_overlay.clone(),
            );

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            connect_sender,
            disconnect_sender,
            start_recording,
            stop_recording,
            get_config,
            update_config,
            poll_events,
            get_stream_port,
            list_mic_devices,
            select_output_dir,
        ])
        .run(tauri::generate_context!())
        .expect("Error while running VRCRecorder");
}
