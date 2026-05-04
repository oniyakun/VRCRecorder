use std::sync::Arc;

use egui_glow::EguiGlow;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

pub struct EguiState {
    pub egui_glow: EguiGlow,
    spout_tex_id: Option<egui::TextureId>,
}

impl EguiState {
    pub fn new(gl: Arc<glow::Context>, event_loop: &ActiveEventLoop) -> Self {
        let egui_glow = EguiGlow::new(event_loop, gl, None, None, false);
        Self {
            egui_glow,
            spout_tex_id: None,
        }
    }

    pub fn on_window_event(
        &mut self,
        window: &Window,
        event: &WindowEvent,
    ) -> bool {
        let resp = self.egui_glow.on_window_event(window, event);
        resp.consumed
    }

    pub fn run(&mut self, window: &Window, run_ui: impl FnMut(&mut egui::Ui)) {
        self.egui_glow.run(window, run_ui);
    }

    pub fn paint(&mut self, window: &Window) {
        self.egui_glow.paint(window);
    }

    /// Register or update the Spout GL texture with egui's painter.
    pub fn update_spout_texture(&mut self, spout_gl_tex: glow::Texture) {
        match self.spout_tex_id {
            Some(id) => {
                self.egui_glow.painter.replace_native_texture(id, spout_gl_tex);
            }
            None => {
                let id = self.egui_glow.painter.register_native_texture(spout_gl_tex);
                self.spout_tex_id = Some(id);
            }
        }
    }

    /// Current Spout texture ID, if registered.
    pub fn spout_tex_id(&self) -> Option<egui::TextureId> {
        self.spout_tex_id
    }

    pub fn destroy(&mut self) {
        self.egui_glow.destroy();
    }
}
