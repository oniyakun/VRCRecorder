use glow::HasContext;

use crate::render::texture::create_input_texture;

/// Renders the Spout source texture into a preview texture with optional
/// mirror (hflip) and portrait (90° CCW rotation) transforms applied via
/// GPU vertex UV manipulation.
pub struct PreviewRenderer {
    vao: glow::VertexArray,
    _vbo: glow::Buffer,
    program: glow::Program,
    fbo: glow::Framebuffer,
    tex: glow::Texture,
    /// Output dimensions (src_w × src_h normally; src_h × src_w when portrait)
    pub width: u32,
    pub height: u32,
}

impl PreviewRenderer {
    /// Create (or recreate) the preview pipeline.
    /// `src_w`, `src_h` are the Spout source texture dimensions.
    pub fn new(
        gl: &glow::Context,
        src_w: u32,
        src_h: u32,
        mirror: bool,
        portrait: bool,
        vflip: bool,
    ) -> Self {
        unsafe {
            // Clean up old resources (handled by caller dropping old Self first)

            // Output dimensions: swap if portrait
            let (out_w, out_h) = if portrait {
                (src_h, src_w)
            } else {
                (src_w, src_h)
            };

            // Output texture
            let tex = create_input_texture(gl, out_w, out_h);

            // FBO
            let fbo = gl.create_framebuffer().unwrap();
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(tex),
                0,
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);

            // Quad with transformed UVs
            let vertices = build_quad_vertices(mirror, portrait, vflip);

            let vao = gl.create_vertex_array().unwrap();
            let vbo = gl.create_buffer().unwrap();
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&vertices),
                glow::STATIC_DRAW,
            );

            let stride = 4 * std::mem::size_of::<f32>() as i32;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(
                1,
                2,
                glow::FLOAT,
                false,
                stride,
                2 * std::mem::size_of::<f32>() as i32,
            );

            let vs = compile_shader(gl, glow::VERTEX_SHADER, VERTEX_SHADER);
            let fs = compile_shader(gl, glow::FRAGMENT_SHADER, FRAGMENT_SHADER);
            let program = link_program(gl, vs, fs);
            gl.delete_shader(vs);
            gl.delete_shader(fs);

            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);

            Self {
                vao,
                _vbo: vbo,
                program,
                fbo,
                tex,
                width: out_w,
                height: out_h,
            }
        }
    }

    /// Render the source Spout texture into the preview texture.
    pub fn render(&self, gl: &glow::Context, src_tex: glow::Texture) {
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            gl.viewport(0, 0, self.width as i32, self.height as i32);

            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(src_tex));
            let loc = gl.get_uniform_location(self.program, "uTexture");
            gl.uniform_1_i32(loc.as_ref(), 0);
            gl.draw_arrays(glow::TRIANGLES, 0, 6);

            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
    }

    pub fn texture(&self) -> glow::Texture {
        self.tex
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self._vbo);
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.tex);
        }
    }
}

/// Vertex data for a fullscreen quad with custom UV coordinates.
fn build_quad_vertices(mirror: bool, portrait: bool, vflip: bool) -> [f32; 24] {
    // Inverse of ffmpeg chain transpose=1,hflip,vflip.
    // Apply operations in reverse: vflip → hflip → transpose, all in output space.
    let transform = |u: f32, v: f32| -> (f32, f32) {
        let v = if vflip { 1.0 - v } else { v }; // vflip first (output space)
        let u = if mirror { 1.0 - u } else { u }; // hflip (output space)
        if portrait {
            (1.0 - v, u) // transpose=2 inverse (90° CCW → source UV)
        } else {
            (u, v)
        }
    };

    // NDC corners: bl, br, tr, tl
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    // Original UVs for each corner
    let uvs = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    // Two triangles
    let tri = [0u32, 1, 2, 0, 2, 3];

    let mut v = [0.0f32; 24];
    for (i, &idx) in tri.iter().enumerate() {
        let base = i * 4;
        let (px, py) = corners[idx as usize];
        let (u, vv) = uvs[idx as usize];
        let (tu, tv) = transform(u, vv);
        v[base] = px;
        v[base + 1] = py;
        v[base + 2] = tu;
        v[base + 3] = tv;
    }
    v
}

// Shaders (same as quad.rs — kept local to avoid making quad internals public)

const VERTEX_SHADER: &str = r#"
#version 330 core
layout(location = 0) in vec2 aPos;
layout(location = 1) in vec2 aTexCoord;
out vec2 vTexCoord;
void main() {
    gl_Position = vec4(aPos, 0.0, 1.0);
    vTexCoord = aTexCoord;
}
"#;

const FRAGMENT_SHADER: &str = r#"
#version 330 core
in vec2 vTexCoord;
out vec4 fragColor;
uniform sampler2D uTexture;
void main() {
    fragColor = texture(uTexture, vTexCoord);
}
"#;

unsafe fn compile_shader(
    gl: &glow::Context,
    shader_type: u32,
    source: &str,
) -> glow::Shader {
    let shader = gl.create_shader(shader_type).unwrap();
    gl.shader_source(shader, source);
    gl.compile_shader(shader);
    if !gl.get_shader_compile_status(shader) {
        let log = gl.get_shader_info_log(shader);
        gl.delete_shader(shader);
        panic!("Preview shader compilation failed: {log}");
    }
    shader
}

unsafe fn link_program(
    gl: &glow::Context,
    vs: glow::Shader,
    fs: glow::Shader,
) -> glow::Program {
    let program = gl.create_program().unwrap();
    gl.attach_shader(program, vs);
    gl.attach_shader(program, fs);
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        let log = gl.get_program_info_log(program);
        gl.delete_program(program);
        panic!("Preview shader linking failed: {log}");
    }
    program
}
