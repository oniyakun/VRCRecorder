use glow::HasContext;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes};

use crate::config::AppConfig;
use crate::record::RecordingPipeline;
use crate::render::gl_context::GlState;
use crate::render::texture;
use crate::spout::receiver::{list_senders, SpoutStream};
use crate::ui::controls::{RecordingAction, RecordingControls};
use crate::ui::egui_state::EguiState;

pub struct App {
    config: AppConfig,

    window: Option<Window>,
    gl_state: Option<GlState>,
    egui_state: Option<EguiState>,

    spout_stream: Option<SpoutStream>,
    spout_tex: Option<glow::Texture>,

    recording_pipeline: RecordingPipeline,
    recording_controls: RecordingControls,
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
            recording_pipeline,
            recording_controls: RecordingControls::new(),
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

    fn render_frame(&mut self) {
        if self.window.is_none() || self.gl_state.is_none() || self.egui_state.is_none() {
            return;
        }

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
                }

                if let Some(ref tex) = self.spout_tex {
                    let tex_raw: u32 = unsafe { std::mem::transmute(*tex) };
                    stream.receive_texture(tex_raw);
                }
            }
        }

        // --- Spout preview texture ---
        let connected = self
            .spout_stream
            .as_ref()
            .map(|s| s.connected)
            .unwrap_or(false);

        if connected {
            if let Some(ref tex) = self.spout_tex {
                self.egui_state
                    .as_mut()
                    .unwrap()
                    .update_spout_texture(*tex);
            }
        }

        let spout_tex_id = self.egui_state.as_mut().unwrap().spout_tex_id();

        let connected_name: Option<String> = self
            .spout_stream
            .as_ref()
            .filter(|s| s.connected)
            .and_then(|s| s.info.as_ref())
            .map(|i| i.name.clone());

        let spout_dims: Option<(u32, u32)> = self
            .spout_stream
            .as_ref()
            .and_then(|s| s.info.as_ref())
            .map(|i| (i.width, i.height));

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
                        let action = self.recording_controls.show(ui);
                        if action != RecordingAction::None {
                            rec_action = action;
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
                    });

                // Center video preview
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    let available = ui.available_size();
                    let (sw, sh) = spout_dims.unwrap_or((1920, 1080));

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
                                ui.image(egui::ImageSource::Texture(
                                    egui::load::SizedTexture::new(
                                        spout_tex_id.unwrap(),
                                        egui::vec2(disp_w, disp_h),
                                    ),
                                ));
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
                let stream = self.spout_stream.as_ref();
                if let Some(stream) = stream {
                    if let Some(ref info) = stream.info {
                        match self
                            .recording_pipeline
                            .start(&gl_arc, info.width, info.height)
                        {
                            Ok(()) => self.recording_controls.on_start(),
                            Err(e) => log::error!("Failed to start recording: {e}"),
                        }
                    }
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
                    let stream = self.spout_stream.as_ref();
                    if let (Some(gl), Some(stream)) = (gl, stream) {
                        if let Some(ref info) = stream.info {
                            let _ = self
                                .recording_pipeline
                                .start(&gl, info.width, info.height);
                            self.recording_controls.on_start();
                        }
                    }
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

    fn shutdown(&mut self) {
        if self.recording_controls.recording {
            let gl = self.gl_state.as_ref().map(|gs| gs.gl().clone());
            if let Some(gl) = gl {
                let _ = self.recording_pipeline.stop(&gl);
            }
        }
        if let Some(egui_state) = self.egui_state.as_mut() {
            egui_state.destroy();
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
