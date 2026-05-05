use glow::HasContext;
use std::sync::mpsc;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes};

use crate::config::AppConfig;
use crate::osc::OscReceiver;
use crate::record::RecordingPipeline;
use crate::render::gl_context::GlState;
use crate::render::preview::PreviewRenderer;
use crate::render::texture;
use crate::spout::receiver::{list_senders, SpoutStream};
use crate::ui::controls::{format_elapsed, RecordingAction, RecordingControls};
use crate::ui::egui_state::EguiState;

enum OscAction {
    StartRecording,
    StopRecording,
}

pub struct App {
    config: AppConfig,

    window: Option<Window>,
    gl_state: Option<GlState>,
    egui_state: Option<EguiState>,

    spout_stream: Option<SpoutStream>,
    spout_tex: Option<glow::Texture>,
    spout_tex_dims: Option<(u32, u32)>,

    recording_pipeline: RecordingPipeline,
    recording_controls: RecordingControls,

    osc_receiver: Option<OscReceiver>,
    osc_rx: Option<mpsc::Receiver<OscAction>>,
    osc_last_enabled: bool,
    osc_last_port: String,
    osc_last_address: String,

    preview_renderer: Option<PreviewRenderer>,
    last_preview_mirror: bool,
    last_preview_portrait: bool,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let recording_pipeline = RecordingPipeline::new(config.output_dir.clone());

        Self {
            config,
            window: None,
            gl_state: None,
            egui_state: None,
            spout_stream: None,
            spout_tex: None,
            spout_tex_dims: None,
            recording_pipeline,
            recording_controls: RecordingControls::new(),
            osc_receiver: None,
            osc_rx: None,
            // Use values that differ from any real config so sync_osc fires on first call
            osc_last_enabled: false,
            osc_last_port: String::new(),
            osc_last_address: String::new(),

            preview_renderer: None,
            last_preview_mirror: false,
            last_preview_portrait: false,
        }
    }

    fn connect_sender(&mut self, name: &str) {
        let gl_arc = self.gl_state.as_ref().map(|gs| gs.gl().clone());
        let mut stream = SpoutStream::new(Some(name));

        if stream.connected {
            if let Some(ref info) = stream.info {
                if let Some(ref gl_arc) = gl_arc {
                    let gl: &glow::Context = gl_arc;
                    self.spout_tex =
                        Some(texture::create_input_texture(gl, info.width, info.height));
                    self.spout_tex_dims = Some((info.width, info.height));
                }
            }
            log::info!("Connected to sender: {name}");
        } else {
            log::warn!("Failed to connect to sender: {name}");
        }

        self.config.last_sender = Some(name.to_string());
        self.config.save();
        self.spout_stream = Some(stream);
    }

    fn sync_osc(&mut self) {
        let enabled = self.config.osc_enabled;
        let port = self.config.osc_port.clone();
        let address = self.config.osc_record_address.clone();

        if enabled == self.osc_last_enabled
            && port == self.osc_last_port
            && address == self.osc_last_address
        {
            return;
        }

        // Stop existing receiver
        if let Some(mut receiver) = self.osc_receiver.take() {
            receiver.stop();
        }

        if enabled {
            let port_num: u16 = match port.parse() {
                Ok(p) => p,
                Err(_) => {
                    log::error!("Invalid OSC port: {port}");
                    return;
                }
            };

            let (tx, rx) = mpsc::channel::<OscAction>();
            let addr_clone = address.clone();
            match OscReceiver::start(port_num, move |msg| {
                if msg.address == addr_clone {
                    let is_start = match msg.value {
                        crate::osc::OscValue::Int(v) => v != 0,
                        crate::osc::OscValue::Bool(v) => v,
                        crate::osc::OscValue::Float(v) => v > 0.5,
                    };
                    let action = if is_start {
                        OscAction::StartRecording
                    } else {
                        OscAction::StopRecording
                    };
                    let _ = tx.send(action);
                }
            }) {
                Ok(receiver) => {
                    self.osc_receiver = Some(receiver);
                    self.osc_rx = Some(rx);
                    log::info!("OSC started: port={port}, address={address}");
                }
                Err(e) => {
                    log::error!("Failed to start OSC: {e}");
                }
            }
        }

        self.osc_last_enabled = enabled;
        self.osc_last_port = port;
        self.osc_last_address = address;
    }

    /// Process any pending OSC actions from the background thread.
    fn process_osc_actions(&mut self) {
        if let Some(ref rx) = self.osc_rx {
            while let Ok(action) = rx.try_recv() {
                let gl = self.gl_state.as_ref().map(|gs| gs.gl().clone());
                match action {
                    OscAction::StartRecording => {
                        if !self.recording_controls.recording {
                            let (w, h, bitrate, mirror, portrait) = self.recording_params();
                            if let Some(gl) = gl {
                                let _ = self.recording_pipeline.start(&gl, w, h, &bitrate, mirror, portrait);
                                self.recording_controls.on_start();
                                self.send_start_camera_osc();
                                log::info!("Recording started via OSC");
                            }
                        }
                    }
                    OscAction::StopRecording => {
                        if self.recording_controls.recording {
                            if let Some(gl) = gl {
                                let _ = self.recording_pipeline.stop(&gl);
                            }
                            self.recording_controls.on_stop();
                            log::info!("Recording stopped via OSC");
                        }
                    }
                }
            }
        }
    }

    fn render_frame(&mut self) {
        if self.window.is_none() || self.gl_state.is_none() || self.egui_state.is_none() {
            return;
        }

        // Sync OSC state with config
        self.sync_osc();

        // Process OSC messages
        self.process_osc_actions();

        let gl_arc = self
            .gl_state
            .as_ref()
            .map(|gs| gs.gl().clone())
            .expect("GL state missing");

        unsafe {
            gl_arc.clear_color(0.1, 0.1, 0.15, 1.0);
            gl_arc.clear(glow::COLOR_BUFFER_BIT);
        }

        // --- Spout update ---
        {
            let stream = self.spout_stream.as_mut();
            if let Some(stream) = stream {
                stream.update();

                if stream.info.is_some() && self.spout_tex.is_none() {
                    let info = stream.info.as_ref().unwrap();
                    self.spout_tex =
                        Some(texture::create_input_texture(&gl_arc, info.width, info.height));
                    self.spout_tex_dims = Some((info.width, info.height));
                }

                // Detect resolution change → restart process
                if let Some(ref info) = stream.info {
                    let new_dims = (info.width, info.height);
                    if self.spout_tex_dims.is_some() && self.spout_tex_dims != Some(new_dims) {
                        log::info!(
                            "Spout resolution changed: {}x{} -> {}x{}, restarting...",
                            self.spout_tex_dims.unwrap().0,
                            self.spout_tex_dims.unwrap().1,
                            new_dims.0,
                            new_dims.1,
                        );
                        // Stop recording cleanly if active
                        if self.recording_pipeline.is_recording() {
                            let _ = self.recording_pipeline.stop(&gl_arc);
                        }
                        self.config.save();
                        let exe = std::env::current_exe().unwrap_or_default();
                        let _ = std::process::Command::new(&exe).spawn();
                        std::process::exit(0);
                    }
                }

                if let Some(ref tex) = self.spout_tex {
                    let tex_raw: u32 = unsafe { std::mem::transmute(*tex) };
                    stream.receive_texture(tex_raw);
                }
            }
        }

        // --- Spout info ---
        let connected = self
            .spout_stream
            .as_ref()
            .map(|s| s.connected)
            .unwrap_or(false);

        let spout_dims: Option<(u32, u32)> = self
            .spout_stream
            .as_ref()
            .and_then(|s| s.info.as_ref())
            .map(|i| (i.width, i.height));

        // --- Preview with mirror/portrait transforms ---
        let mirror = self.config.mirror_video;
        let portrait = self.config.portrait_mode;

        if connected {
            if let Some(ref tex) = self.spout_tex {
                if mirror || portrait {
                    if let Some((sw, sh)) = spout_dims {
                        let mode_changed =
                            mirror != self.last_preview_mirror || portrait != self.last_preview_portrait;
                        let cur_dims = self
                            .preview_renderer
                            .as_ref()
                            .map(|p| (p.width, p.height));
                        let expect_dims = if portrait { (sh, sw) } else { (sw, sh) };

                        if mode_changed || cur_dims != Some(expect_dims) {
                            if let Some(old) = self.preview_renderer.take() {
                                old.destroy(&gl_arc);
                            }
                            self.preview_renderer = Some(PreviewRenderer::new(
                                &gl_arc, sw, sh, mirror, portrait,
                            ));
                            self.last_preview_mirror = mirror;
                            self.last_preview_portrait = portrait;
                        }

                        if let Some(ref preview) = self.preview_renderer {
                            preview.render(&gl_arc, *tex);
                            self.egui_state
                                .as_mut()
                                .unwrap()
                                .update_spout_texture(preview.texture());
                        }
                    }
                } else {
                    // No transform — use raw Spout texture, drop any old preview
                    if self.preview_renderer.is_some() {
                        if let Some(old) = self.preview_renderer.take() {
                            old.destroy(&gl_arc);
                        }
                    }
                    self.egui_state
                        .as_mut()
                        .unwrap()
                        .update_spout_texture(*tex);
                }
            }
        } else if self.preview_renderer.is_some() {
            // Disconnected — clean up preview
            if let Some(old) = self.preview_renderer.take() {
                old.destroy(&gl_arc);
            }
        }

        let spout_tex_id = self.egui_state.as_mut().unwrap().spout_tex_id();

        let connected_name: Option<String> = self
            .spout_stream
            .as_ref()
            .filter(|s| s.connected)
            .and_then(|s| s.info.as_ref())
            .map(|i| i.name.clone());

        let senders = list_senders();

        // --- Recording update ---
        if self.recording_pipeline.is_recording() {
            if let Some(ref tex) = self.spout_tex {
                self.recording_pipeline.process_frame(&gl_arc, *tex);
            }
            self.recording_controls.update();
        }

        // --- Build egui UI ---
        let mut rec_action = RecordingAction::None;
        let mut connect_request: Option<String> = None;

        {
            let egui_state = self.egui_state.as_mut().unwrap();
            let window = self.window.as_ref().unwrap();
            egui_state.run(window, |ui| {
                // Bottom recording bar
                egui::TopBottomPanel::bottom("recording_bar").show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        if self.recording_controls.recording {
                            let stop_btn = egui::Button::new(
                                egui::RichText::new("⏹ Stop").color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::DARK_RED)
                            .min_size(egui::vec2(80.0, 32.0));
                            if ui.add(stop_btn).clicked() {
                                rec_action = RecordingAction::Stop;
                            }
                            // Red dot
                            let time = ui.input(|i| i.time);
                            let alpha = ((time * 3.0).sin() * 0.5 + 0.5) as f32;
                            ui.colored_label(
                                egui::Color32::from_rgba_premultiplied(255, 0, 0, (alpha * 255.0) as u8),
                                "●",
                            );
                            ui.label(format_elapsed(self.recording_controls.elapsed));
                        } else {
                            let rec_btn = egui::Button::new(
                                egui::RichText::new("● REC").color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::DARK_RED)
                            .min_size(egui::vec2(80.0, 32.0));
                            if ui.add(rec_btn).clicked() {
                                rec_action = RecordingAction::Start;
                            }
                        }

                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if let Some((w, h)) = spout_dims {
                                    ui.label(format!("{w}x{h}"));
                                }
                            },
                        );
                    });
                });

                // Right sender panel
                egui::SidePanel::right("sender_panel")
                    .resizable(true)
                    .default_width(250.0)
                    .show_inside(ui, |ui| {
                        ui.heading("Senders");
                        ui.separator();

                        if senders.is_empty() {
                            ui.label(
                                egui::RichText::new("No senders found")
                                    .color(ui.visuals().weak_text_color()),
                            );
                        }

                        for name in &senders {
                            let is_connected =
                                connected_name.as_deref() == Some(name.as_str());
                            let label = if is_connected {
                                egui::RichText::new(format!("● {name}")).strong()
                            } else {
                                egui::RichText::new(format!("○ {name}"))
                            };

                            let clicked = ui
                                .add(egui::Label::new(label).sense(egui::Sense::click()))
                                .clicked();

                            if clicked {
                                log::info!("Sender clicked: {name}");
                                connect_request = Some(name.clone());
                            }
                        }

                        ui.separator();
                        ui.collapsing("Status", |ui| {
                            if let Some(ref name) = connected_name {
                                ui.label(format!("Connected: {name}"));
                                if let Some((w, h)) = spout_dims {
                                    ui.label(format!("Resolution: {w}x{h}"));
                                }
                            } else {
                                ui.label(
                                    egui::RichText::new("Not connected")
                                        .color(ui.visuals().warn_fg_color),
                                );
                            }
                        });

                        ui.separator();
                        ui.collapsing("OSC (VRChat)", |ui| {
                            if ui.checkbox(&mut self.config.osc_enabled, "Enable OSC receiver").changed() {
                                self.config.save();
                            }
                            ui.add_enabled_ui(self.config.osc_enabled, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Listen port:");
                                    if ui.text_edit_singleline(&mut self.config.osc_port).changed() {
                                        self.config.save();
                                    }
                                });
                                ui.label("Record address:");
                                if ui.text_edit_singleline(&mut self.config.osc_record_address).changed() {
                                    self.config.save();
                                }
                            });
                            ui.separator();
                            if ui.checkbox(&mut self.config.auto_camera, "Auto camera + Spout on record").changed() {
                                self.config.save();
                            }
                            ui.add_enabled_ui(self.config.auto_camera, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("VRChat port:");
                                    if ui.text_edit_singleline(&mut self.config.vrchat_port).changed() {
                                        self.config.save();
                                    }
                                });
                            });
                        });

                        ui.separator();
                        ui.collapsing("Video", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Bitrate:");
                                egui::ComboBox::from_id_salt("video_bitrate")
                                    .selected_text(&self.config.video_bitrate)
                                    .show_ui(ui, |ui| {
                                        for rate in ["2M", "4M", "8M", "20M"] {
                                            if ui.selectable_value(&mut self.config.video_bitrate, rate.to_string(), rate).changed() {
                                                self.config.save();
                                            }
                                        }
                                    });
                            });
                            if ui.checkbox(&mut self.config.mirror_video, "Mirror (hflip)").changed() {
                                self.config.save();
                            }
                            if ui.checkbox(&mut self.config.portrait_mode, "Portrait mode (rotate 90°)").changed() {
                                self.config.save();
                            }
                        });
                    });

                // Center video preview
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    let available = ui.available_size();
                    // Use preview renderer dims if active (already transformed), else raw spout dims
                    let (sw, sh) = if let Some(ref preview) = self.preview_renderer {
                        (preview.width, preview.height)
                    } else {
                        spout_dims.unwrap_or((1920, 1080))
                    };

                    if sw > 0 && sh > 0 && available.x > 0.0 && available.y > 0.0 {
                        let aspect = sw as f32 / sh as f32;
                        let avail_aspect = available.x / available.y;

                        let (disp_w, disp_h) = if aspect > avail_aspect {
                            (available.x, available.x / aspect)
                        } else {
                            (available.y * aspect, available.y)
                        };

                        ui.centered_and_justified(|ui| {
                            if connected && spout_tex_id.is_some() {
                                // Preview texture already has mirror/portrait baked in via GPU
                                ui.add(egui::Image::new(egui::ImageSource::Texture(
                                    egui::load::SizedTexture::new(
                                        spout_tex_id.unwrap(),
                                        egui::vec2(disp_w, disp_h),
                                    ),
                                )));
                            } else {
                                let rect = egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(disp_w, disp_h),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    0.0,
                                    egui::Color32::from_black_alpha(200),
                                );
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "No Signal",
                                    egui::FontId::proportional(24.0),
                                    ui.visuals().weak_text_color(),
                                );
                            }
                        });
                    }
                });
            });
        }

        // --- Handle UI actions ---
        if let Some(name) = connect_request {
            self.connect_sender(&name);
        }

        match rec_action {
            RecordingAction::Start => {
                let (w, h, bitrate, mirror, portrait) = self.recording_params();

                match self.recording_pipeline.start(&gl_arc, w, h, &bitrate, mirror, portrait) {
                    Ok(()) => {
                        self.recording_controls.on_start();
                        // Send camera OSC in background — don't delay recording start
                        self.send_start_camera_osc();
                    }
                    Err(e) => log::error!("Failed to start recording: {e}"),
                }
            }
            RecordingAction::Stop => {
                let _ = self.recording_pipeline.stop(&gl_arc);
                self.recording_controls.on_stop();
            }
            RecordingAction::None => {}
        }

        // --- Paint ---
        {
            let window = self.window.as_ref().unwrap();
            self.egui_state.as_mut().unwrap().paint(window);
        }

        let _ = self.gl_state.as_ref().unwrap().swap_buffers();
    }

    fn handle_keypress(&mut self, key: Key) {
        match key {
            Key::Named(NamedKey::Space) => {
                if self.recording_controls.recording {
                    let gl = self.gl_state.as_ref().map(|gs| gs.gl().clone());
                    if let Some(gl) = gl {
                        let _ = self.recording_pipeline.stop(&gl);
                    }
                    self.recording_controls.on_stop();
                } else {
                    let gl = self.gl_state.as_ref().map(|gs| gs.gl().clone());
                    let (w, h, bitrate, mirror, portrait) = self.recording_params();
                    let _ = self.recording_pipeline.start(&gl.unwrap(), w, h, &bitrate, mirror, portrait);
                    self.recording_controls.on_start();
                    self.send_start_camera_osc();
                }
            }
            Key::Named(NamedKey::Escape) => {
                if self.recording_controls.recording {
                    let gl = self.gl_state.as_ref().map(|gs| gs.gl().clone());
                    if let Some(gl) = gl {
                        let _ = self.recording_pipeline.stop(&gl);
                    }
                    self.recording_controls.on_stop();
                }
            }
            _ => {}
        }
    }

    fn recording_params(&self) -> (u32, u32, String, bool, bool) {
        let (w, h) = self
            .spout_stream
            .as_ref()
            .and_then(|s| s.info.as_ref())
            .map(|i| (i.width, i.height))
            .unwrap_or((1920, 1080));
        (
            w,
            h,
            self.config.video_bitrate.clone(),
            self.config.mirror_video,
            self.config.portrait_mode,
        )
    }

    fn send_start_camera_osc(&self) {
        if !self.config.auto_camera {
            return;
        }
        let port: u16 = match self.config.vrchat_port.parse() {
            Ok(p) => p,
            Err(_) => return,
        };
        std::thread::spawn(move || {
            crate::osc::send_osc(port, "/usercamera/Streaming", &rosc::OscType::Bool(true)).ok();
            log::info!("OSC: Spout enabled");
        });
    }

    fn shutdown(&mut self) {
        if self.recording_controls.recording {
            let gl = self.gl_state.as_ref().map(|gs| gs.gl().clone());
            if let Some(gl) = gl {
                let _ = self.recording_pipeline.stop(&gl);
            }
        }
        if let Some(mut receiver) = self.osc_receiver.take() {
            receiver.stop();
        }
        if let Some(egui_state) = self.egui_state.as_mut() {
            egui_state.destroy();
        }
        if let Some(gl) = self.gl_state.as_ref().map(|gs| gs.gl().clone()) {
            if let Some(preview) = self.preview_renderer.take() {
                preview.destroy(&gl);
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window_attrs = WindowAttributes::default()
            .with_title("VRCRecorder")
            .with_inner_size(PhysicalSize::new(
                self.config.window_width,
                self.config.window_height,
            ));

        let (window, gl_state) =
            GlState::new(window_attrs, event_loop).expect("Failed to create OpenGL window");

        let gl = gl_state.gl().clone();
        let egui_state = EguiState::new(gl, event_loop);

        self.window = Some(window);
        self.gl_state = Some(gl_state);
        self.egui_state = Some(egui_state);

        if let Some(ref window) = self.window {
            window.request_redraw();
        }

        if let Some(ref name) = self.config.last_sender.clone() {
            self.connect_sender(name);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        let handled = {
            if let Some(egui_state) = self.egui_state.as_mut() {
                egui_state.on_window_event(window, &event)
            } else {
                false
            }
        };
        if handled {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                self.shutdown();
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {}
            WindowEvent::Resized(size) => {
                if let Some(gl_state) = self.gl_state.as_mut() {
                    gl_state.resize(size);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key: key,
                        ..
                    },
                ..
            } => {
                self.handle_keypress(key);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.render_frame();
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }
}
