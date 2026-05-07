use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use glow::HasContext;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{WindowAttributes, WindowId};

use crate::channels::{RenderCommand, RenderEvent};
use crate::config::AppConfig;
use crate::render::gl_context::GlState;
use crate::render::preview::PreviewRenderer;
use crate::render::texture;
use crate::record::pipeline::RecordingPipeline;
use crate::spout::receiver::{list_senders, SpoutStream};

/// Spawn the render thread with a hidden GL window.
pub fn spawn(
    mut config: AppConfig,
    cmd_rx: mpsc::Receiver<RenderCommand>,
    event_tx: mpsc::Sender<RenderEvent>,
    preview_buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    preview_dims: std::sync::Arc<std::sync::Mutex<(u32, u32)>>,
    is_recording: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("render-thread".into())
        .spawn(move || {
            let event_loop = winit::event_loop::EventLoop::builder()
                .with_any_thread(true)
                .build()
                .expect("Failed to create render event loop");
            let mut state = RenderState::new(config, cmd_rx, event_tx, preview_buf, preview_dims, is_recording);
            let _ = event_loop.run_app(&mut state);
        })
        .expect("Failed to spawn render thread")
}

struct RenderState {
    config: AppConfig,
    cmd_rx: mpsc::Receiver<RenderCommand>,
    event_tx: mpsc::Sender<RenderEvent>,

    // GL infrastructure
    gl_state: Option<GlState>,
    gl: Option<Arc<glow::Context>>,

    // Spout
    spout_stream: Option<SpoutStream>,
    spout_tex: Option<glow::Texture>,
    spout_tex_dims: Option<(u32, u32)>,

    // Preview
    preview_renderer: Option<PreviewRenderer>,
    preview_fbo: Option<glow::Framebuffer>,
    preview_buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    preview_dims_shared: std::sync::Arc<std::sync::Mutex<(u32, u32)>>,
    last_preview_mirror: bool,
    last_preview_portrait: bool,
    last_preview_vflip: bool,

    // Recording
    recording_pipeline: RecordingPipeline,
    recording: bool,
    is_recording: Arc<AtomicBool>,
    recording_start: Option<Instant>,

    // Preview delivery
    last_preview_sent: Option<Instant>,

    // Sender scanning
    last_sender_scan: Option<Instant>,
    last_sender_list: Vec<String>,

    // Periodic emit
    last_elapsed_emit: Option<Instant>,
    last_mic_check: Option<Instant>,
}

impl RenderState {
    fn new(
        config: AppConfig,
        cmd_rx: mpsc::Receiver<RenderCommand>,
        event_tx: mpsc::Sender<RenderEvent>,
        preview_buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
        preview_dims_shared: std::sync::Arc<std::sync::Mutex<(u32, u32)>>,
        is_recording: Arc<AtomicBool>,
    ) -> Self {
        let output_dir = config.output_dir.clone();
        Self {
            config,
            cmd_rx,
            event_tx,
            gl_state: None,
            gl: None,
            spout_stream: None,
            spout_tex: None,
            spout_tex_dims: None,
            preview_renderer: None,
            preview_fbo: None,
            preview_buf,
            preview_dims_shared,
            last_preview_mirror: false,
            last_preview_portrait: false,
            last_preview_vflip: false,
            recording_pipeline: RecordingPipeline::new(output_dir),
            recording: false,
            is_recording,
            recording_start: None,
            last_preview_sent: None,
            last_sender_scan: None,
            last_sender_list: Vec::new(),
            last_elapsed_emit: None,
            last_mic_check: None,
        }
    }

    // ---- Command processing ----

    fn process_commands(&mut self) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                RenderCommand::ConnectSender(name) => self.connect_sender(&name),
                RenderCommand::DisconnectSender => self.disconnect_sender(),
                RenderCommand::StartRecording => self.start_recording(),
                RenderCommand::StopRecording => self.stop_recording(),
                RenderCommand::UpdateConfig { bitrate, mirror, portrait, vflip, preview_enabled, mic_enabled, mic_device, auto_stop_camera } => {
                    let old_preview = self.config.preview_enabled;
                    self.config.video_bitrate = bitrate;
                    self.config.mirror_video = mirror;
                    self.config.portrait_mode = portrait;
                    self.config.video_vflip = vflip;
                    self.config.preview_enabled = preview_enabled;
                    self.config.mic_enabled = mic_enabled;
                    self.config.mic_device = mic_device;
                    self.config.auto_stop_camera = auto_stop_camera;
                    // Toggle camera Spout output when preview changed and not recording
                    if old_preview != preview_enabled && !self.recording {
                        self.send_spout_streaming_osc(preview_enabled);
                    }
                }
                RenderCommand::UpdateOrientation { is_landscape } => {
                    let portrait = !is_landscape;
                    if self.config.portrait_mode != portrait {
                        self.config.portrait_mode = portrait;
                        self.config.save();
                        log::info!("OSC orientation → portrait={portrait}");
                        let _ = self.event_tx.send(RenderEvent::ConfigChanged {
                            mirror_video: self.config.mirror_video,
                            portrait_mode: self.config.portrait_mode,
                            video_vflip: self.config.video_vflip,
                            preview_enabled: self.config.preview_enabled,
                            mic_enabled: self.config.mic_enabled,
                        });
                    }
                }
                RenderCommand::Shutdown => self.shutdown(),
            }
        }
    }

    // ---- Sender management ----

    fn connect_sender(&mut self, name: &str) {
        self.config.last_sender = Some(name.to_string());
        self.config.save();
        if let Some(ref gl_arc) = self.gl {
            let gl: &glow::Context = gl_arc.as_ref();
            let stream = SpoutStream::new(Some(name));
            if let Some(ref info) = stream.info {
                self.spout_tex =
                    Some(texture::create_input_texture(gl, info.width, info.height));
                self.spout_tex_dims = Some((info.width, info.height));
            }
            if stream.connected {
                if let Some(ref info) = stream.info {
                    let _ = self.event_tx.send(RenderEvent::SenderConnected {
                        name: name.to_string(),
                        width: info.width,
                        height: info.height,
                    });
                }
                log::info!("Connected to sender: {name}");
            }
            self.spout_stream = Some(stream);
        }
    }

    fn disconnect_sender(&mut self) {
        self.spout_stream = None;
        self.spout_tex = None;
        self.spout_tex_dims = None;
        if let Some(gl) = self.gl.as_ref() {
            if let Some(old) = self.preview_renderer.take() {
                old.destroy(gl);
            }
        }
        self.config.last_sender = None;
        self.config.save();
        let _ = self.event_tx.send(RenderEvent::SenderDisconnected);
        log::info!("Disconnected from sender");
    }

    // ---- Recording ----

    fn recording_params(&self) -> (u32, u32, String, bool, bool, bool) {
        let (w, h) = self
            .spout_stream
            .as_ref()
            .and_then(|s| s.info.as_ref())
            .map(|i| (i.width, i.height))
            .unwrap_or((1920, 1080));
        (
            w, h,
            self.config.video_bitrate.clone(),
            self.config.mirror_video,
            self.config.portrait_mode,
            self.config.video_vflip,
        )
    }

    fn start_recording(&mut self) {
        if self.recording { return; }
        // Auto-disable preview to save resources for recording
        if self.config.preview_enabled {
            self.config.preview_enabled = false;
            // Clean up preview GPU resources immediately
            if let Some(gl) = self.gl.as_ref() {
                if let Some(old) = self.preview_renderer.take() {
                    old.destroy(gl);
                }
            }
            let _ = self.event_tx.send(RenderEvent::ConfigChanged {
                mirror_video: self.config.mirror_video,
                portrait_mode: self.config.portrait_mode,
                video_vflip: self.config.video_vflip,
                preview_enabled: self.config.preview_enabled,
                mic_enabled: self.config.mic_enabled,
            });
        }
        // Send camera OSC in background
        self.send_camera_osc();
        let (w, h, bitrate, mirror, portrait, vflip) = self.recording_params();
        if let Some(gl) = self.gl.as_ref() {
            match self.recording_pipeline.start(
                gl, w, h, &bitrate, mirror, portrait, vflip,
                self.config.mic_enabled,
                self.config.mic_device.as_deref(),
            ) {
                Ok(()) => {
                    self.recording = true;
                    self.is_recording.store(true, Ordering::Relaxed);
                    self.recording_start = Some(Instant::now());
                    let _ = self.event_tx.send(RenderEvent::RecordingStarted);
                    log::info!("Recording started");
                }
                Err(e) => {
                    let _ = self.event_tx.send(RenderEvent::Error { message: e.clone() });
                    log::error!("Failed to start recording: {e}");
                }
            }
        }
    }

    fn stop_recording(&mut self) {
        if !self.recording { return; }
        if let Some(gl) = self.gl.as_ref() {
            let _ = self.recording_pipeline.stop(gl);
        }
        self.recording = false;
        self.is_recording.store(false, Ordering::Relaxed);
        self.recording_start = None;
        self.send_camera_stop_osc();
        let _ = self.event_tx.send(RenderEvent::RecordingStopped {
            final_path: None,
        });
        log::info!("Recording stopped");
    }

    fn send_camera_osc(&self) {
        if !self.config.auto_camera { return; }
        self.send_spout_streaming_osc(true);
    }

    fn send_camera_stop_osc(&self) {
        if !self.config.auto_stop_camera { return; }
        self.send_spout_streaming_osc(false);
    }

    fn send_spout_streaming_osc(&self, enabled: bool) {
        let port: u16 = match self.config.vrchat_port.parse() {
            Ok(p) => p,
            Err(_) => return,
        };
        std::thread::spawn(move || {
            crate::osc::send_osc(port, "/usercamera/Streaming", &rosc::OscType::Bool(enabled)).ok();
            log::info!("OSC: Spout {}", if enabled { "enabled" } else { "disabled" });
        });
    }

    fn shutdown(&self) {
        log::info!("Render thread shutting down");
        // Recording pipeline will be dropped, which cleans up
    }

    // ---- Per-frame updates ----

    /// Returns true if a new Spout frame was received into our GL texture.
    fn update_spout(&mut self) -> bool {
        let gl_arc = match &self.gl {
            Some(gl) => gl.clone(),
            None => return false,
        };

        let stream = match &mut self.spout_stream {
            Some(s) => s,
            None => return false,
        };

        stream.update();

        // Create texture on first connection
        if stream.info.is_some() && self.spout_tex.is_none() {
            let info = stream.info.as_ref().unwrap();
            self.spout_tex =
                Some(texture::create_input_texture(&gl_arc, info.width, info.height));
            self.spout_tex_dims = Some((info.width, info.height));
        }

        // Detect resolution change
        if let Some(ref info) = stream.info {
            let new_dims = (info.width, info.height);
            if self.spout_tex_dims.is_some() && self.spout_tex_dims != Some(new_dims) {
                let (old_w, old_h) = self.spout_tex_dims.unwrap();
                log::info!("Spout resolution changed: {old_w}x{old_h} -> {}x{}", new_dims.0, new_dims.1);

                // Stop recording if active
                if self.recording {
                    if let Some(gl) = self.gl.as_ref() {
                        let _ = self.recording_pipeline.stop(gl);
                    }
                    self.recording = false;
                    self.recording_start = None;
                }

                self.config.save();
                let _ = self.event_tx.send(RenderEvent::ResolutionChanged {
                    old_w, old_h,
                    new_w: new_dims.0,
                    new_h: new_dims.1,
                });
                return false;
            }
        }

        // Receive Spout frame into GL texture
        if let Some(ref tex) = self.spout_tex {
            let tex_raw: u32 = unsafe { std::mem::transmute(*tex) };
            stream.receive_texture(tex_raw)
        } else {
            false
        }
    }

    fn update_preview(&mut self) {
        let now = Instant::now();
        let gl = match &self.gl {
            Some(gl) => gl.as_ref(),
            None => return,
        };

        let spout_tex = match self.spout_tex {
            Some(t) => t,
            None => return,
        };

        let (sw, sh) = match self
            .spout_stream
            .as_ref()
            .and_then(|s| s.info.as_ref())
            .map(|i| (i.width, i.height))
        {
            Some(d) => d,
            None => return,
        };

        // No throttle — every new frame gets sent for preview

        let mirror = self.config.mirror_video;
        let portrait = self.config.portrait_mode;
        let vflip = self.config.video_vflip;

        // Cap preview at 1080p (or smaller if source is smaller)
        let max_dim = 1920u32.max(1080u32);
        let (pw, ph) = if sw > max_dim || sh > max_dim {
            let scale = max_dim as f32 / sw.max(sh) as f32;
            ((sw as f32 * scale) as u32, (sh as f32 * scale) as u32)
        } else {
            (sw, sh)
        };

        // Rebuild PreviewRenderer if mode or size changed
        let mode_changed = mirror != self.last_preview_mirror
            || portrait != self.last_preview_portrait
            || vflip != self.last_preview_vflip;

        let (exp_w, exp_h) = if portrait { (ph, pw) } else { (pw, ph) };
        let size_changed = match &self.preview_renderer {
            Some(p) => p.width != exp_w || p.height != exp_h,
            None => true,
        };

        if mode_changed || size_changed {
            if let Some(old) = self.preview_renderer.take() {
                old.destroy(gl);
            }
            self.preview_renderer = Some(PreviewRenderer::new(
                gl, pw, ph, mirror, portrait, vflip,
            ));
            self.last_preview_mirror = mirror;
            self.last_preview_portrait = portrait;
            self.last_preview_vflip = vflip;
        }

        let preview = self.preview_renderer.as_ref().unwrap();
        preview.render(gl, spout_tex);

        // Read pixels via persistent FBO — use preview output dims
        // (may differ from pw×ph when portrait swaps width/height)
        let out_w = preview.width;
        let out_h = preview.height;

        if self.preview_fbo.is_none() {
            unsafe {
                self.preview_fbo = Some(gl.create_framebuffer().unwrap());
            }
        }

        let size = (out_w * out_h * 4) as usize;
        let mut pixels = vec![0u8; size];
        unsafe {
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.preview_fbo);
            gl.framebuffer_texture_2d(
                glow::READ_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(preview.texture()),
                0,
            );

            gl.read_pixels(
                0, 0,
                out_w as i32, out_h as i32,
                glow::RGBA, glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels[..])),
            );
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
        }

        // JPEG encode and write to shared buffer
        let rgb: Vec<u8> = pixels
            .chunks(4)
            .flat_map(|c| [c[0], c[1], c[2]])
            .collect();

        let mut jpeg_buf = Vec::new();
        {
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, 80);
            use image::ImageEncoder;
            if let Err(e) = encoder.write_image(&rgb, out_w, out_h, image::ExtendedColorType::Rgb8) {
                log::error!("Preview JPEG encode failed: {e}");
                return;
            }
        }

        {
            let mut buf = self.preview_buf.lock().unwrap();
            *buf = jpeg_buf;
        }
        {
            let mut dims = self.preview_dims_shared.lock().unwrap();
            *dims = (out_w, out_h);
        }

        self.last_preview_sent = Some(now);
    }

    fn process_recording(&mut self) {
        if !self.recording { return; }
        let gl = match &self.gl {
            Some(gl) => gl.as_ref(),
            None => return,
        };
        if let Some(ref tex) = self.spout_tex {
            self.recording_pipeline.process_frame(gl, *tex);
        }

        // Periodic mic device change check (every 2s)
        let now = Instant::now();
        if self.config.mic_enabled {
            if let Some(last) = self.last_mic_check {
                if (now - last).as_secs() >= 2 {
                    self.recording_pipeline.check_mic_device();
                    self.last_mic_check = Some(now);
                }
            } else {
                self.last_mic_check = Some(now);
            }
        }
    }

    fn scan_senders(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_sender_scan {
            if (now - last).as_secs() < 2 {
                return;
            }
        }

        let senders = list_senders();
        if senders != self.last_sender_list {
            let _ = self.event_tx.send(RenderEvent::SenderListUpdated {
                senders: senders.clone(),
            });
            self.last_sender_list = senders.clone();
        }

        // Auto-connect to first available sender if not already connected
        if self.spout_stream.is_none() && !senders.is_empty() {
            // Try saved last_sender first, otherwise first in list
            let target = self
                .config
                .last_sender
                .clone()
                .filter(|n| senders.contains(n))
                .unwrap_or_else(|| senders[0].clone());
            log::info!("Auto-connecting to sender: {target}");
            self.connect_sender(&target);
        }
        self.last_sender_scan = Some(now);
    }

    fn emit_elapsed(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_elapsed_emit {
            if (now - last).as_secs() < 1 {
                return;
            }
        }

        if let Some(start) = self.recording_start {
            let elapsed = start.elapsed().as_secs();
            let _ = self.event_tx.send(RenderEvent::RecordingElapsed {
                elapsed_secs: elapsed,
            });
        }
        self.last_elapsed_emit = Some(now);
    }
}

impl ApplicationHandler for RenderState {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gl_state.is_some() {
            return;
        }

        // Create hidden 1x1 window for GPU compute
        let window_attrs = WindowAttributes::default()
            .with_visible(false)
            .with_inner_size(PhysicalSize::new(1, 1));

        let (_, gl_state) = match GlState::new(window_attrs, event_loop) {
            Ok(v) => v,
            Err(e) => {
                log::error!("Failed to create hidden GL window: {e}");
                let _ = self.event_tx.send(RenderEvent::Error {
                    message: format!("GL init failed: {e}"),
                });
                event_loop.exit();
                return;
            }
        };

        self.gl = Some(gl_state.gl().clone());
        self.gl_state = Some(gl_state);

        // Connect to last sender if configured
        if let Some(ref name) = self.config.last_sender.clone() {
            self.connect_sender(name);
        }

        log::info!("Render thread initialized — hidden GL window ready");
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Wake up every 16ms (~60fps) to check for new Spout frames.
        // Avoids ControlFlow::Poll which burns 100% CPU.
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + std::time::Duration::from_millis(16),
        ));

        // 1. Spout update — returns true if a new frame was received
        //    MUST run before process_commands so that when StopRecording is
        //    processed, the latest frame has already been captured into the
        //    PBO pipeline and flush() can drain it.
        let new_frame = self.update_spout();

        // 2. Only do heavy GPU work on new frames
        if new_frame {
            // Preview (only when enabled)
            if self.config.preview_enabled {
                self.update_preview();
            }

            // Recording pipeline
            self.process_recording();
        }

        // 3. Process incoming commands (after frame capture)
        self.process_commands();

        // 4. Periodic sender scan (every 2s)
        self.scan_senders();

        // 5. Emit elapsed time (every 1s)
        self.emit_elapsed();

        // 6. Swap buffers (required even for hidden window)
        if let Some(ref gl_state) = self.gl_state {
            let _ = gl_state.swap_buffers();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::CloseRequested = event {
            if self.recording {
                if let Some(gl) = self.gl.as_ref() {
                    let _ = self.recording_pipeline.stop(gl);
                }
            }
            if let Some(gl) = self.gl.as_ref() {
                if let Some(p) = self.preview_renderer.take() {
                    p.destroy(gl);
                }
            }
            event_loop.exit();
        }
    }
}
