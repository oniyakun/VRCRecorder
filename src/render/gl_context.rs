use std::ffi::CString;
use std::num::NonZeroU32;
use std::sync::Arc;

use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, WindowSurface};
use raw_window_handle::HasWindowHandle;
use winit::window::Window;

pub struct GlState {
    pub gl: Arc<glow::Context>,
    display: glutin::display::Display,
    context: glutin::context::PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
    pub window_size: (u32, u32),
    window_handle: raw_window_handle::RawWindowHandle,
}

impl GlState {
    pub fn new(
        window_attrs: winit::window::WindowAttributes,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> Result<(Window, Self), Box<dyn std::error::Error>> {
        let template = glutin::config::ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_transparency(false);

        let display_builder =
            glutin_winit::DisplayBuilder::new().with_window_attributes(Some(window_attrs));

        let (window, gl_config) = display_builder
            .build(event_loop, template, |configs| {
                configs
                    .reduce(|a, b| {
                        if a.num_samples() > b.num_samples() {
                            a
                        } else {
                            b
                        }
                    })
                    .unwrap()
            })
            .map_err(|e| format!("Failed to create display: {e}"))?;

        let window = window.ok_or("Failed to create window")?;
        let display = gl_config.display();
        let window_handle = window.window_handle()?.as_raw();

        let (w, h) = {
            let size = window.inner_size();
            (size.width.max(1), size.height.max(1))
        };

        // Create surface first
        let surface_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            window_handle,
            NonZeroU32::new(w).unwrap(),
            NonZeroU32::new(h).unwrap(),
        );

        let surface = unsafe {
            display
                .create_window_surface(&gl_config, &surface_attrs)
                .map_err(|e| format!("Failed to create window surface: {e}"))?
        };

        // Create GL context and make it current on the surface
        let context_attrs = glutin::context::ContextAttributesBuilder::new()
            .with_context_api(glutin::context::ContextApi::OpenGl(Some(
                glutin::context::Version::new(4, 6),
            )))
            .build(Some(window_handle));

        let not_current_ctx = unsafe {
            display
                .create_context(&gl_config, &context_attrs)
                .map_err(|e| format!("Failed to create GL context: {e}"))?
        };

        let context = not_current_ctx
            .make_current(&surface)
            .map_err(|e| format!("Failed to make context current: {e}"))?;

        let gl = unsafe {
            Arc::new(glow::Context::from_loader_function(|s| {
                let c_str = CString::new(s).unwrap_or_default();
                display.get_proc_address(&c_str)
            }))
        };

        Ok((
            window,
            Self {
                gl,
                display: display.clone(),
                context,
                surface,
                window_size: (w, h),
                window_handle,
            },
        ))
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.window_size = (new_size.width, new_size.height);
        if new_size.width > 0 && new_size.height > 0 {
            let w = NonZeroU32::new(new_size.width).unwrap();
            let h = NonZeroU32::new(new_size.height).unwrap();
            self.surface.resize(&self.context, w, h);
            unsafe {
                use glow::HasContext as _;
                self.gl
                    .viewport(0, 0, new_size.width as i32, new_size.height as i32);
            }
        }
    }

    pub fn swap_buffers(&self) -> Result<(), glutin::error::Error> {
        self.surface.swap_buffers(&self.context)
    }

    pub fn gl(&self) -> &Arc<glow::Context> {
        &self.gl
    }
}

impl Drop for GlState {
    fn drop(&mut self) {
        unsafe {
            use glow::HasContext as _;
            self.gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
        }
    }
}
