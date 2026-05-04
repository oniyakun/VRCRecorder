use glow::HasContext;

// Simple fullscreen quad shaders
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

pub struct FullscreenQuad {
    vao: glow::VertexArray,
    _vbo: glow::Buffer,
    program: glow::Program,
}

impl FullscreenQuad {
    pub fn new(gl: &glow::Context) -> Self {
        unsafe {
            let vao = gl.create_vertex_array().unwrap();
            let vbo = gl.create_buffer().unwrap();
            gl.bind_vertex_array(Some(vao));

            // Two triangles covering NDC [-1,1] with texcoords [0,1]
            #[rustfmt::skip]
            let vertices: [f32; 24] = [
                // pos(x,y)    tex(u,v)
                -1.0, -1.0,    0.0, 0.0,
                 1.0, -1.0,    1.0, 0.0,
                 1.0,  1.0,    1.0, 1.0,

                -1.0, -1.0,    0.0, 0.0,
                 1.0,  1.0,    1.0, 1.0,
                -1.0,  1.0,    0.0, 1.0,
            ];

            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&vertices),
                glow::STATIC_DRAW,
            );

            let stride = 4 * std::mem::size_of::<f32>() as i32;
            // Position
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
            // TexCoord
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, 2 * std::mem::size_of::<f32>() as i32);

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
            }
        }
    }

    /// Render the given texture fullscreen.
    pub fn render(&self, gl: &glow::Context, tex: glow::Texture) {
        unsafe {
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            let loc = gl.get_uniform_location(self.program, "uTexture");
            gl.uniform_1_i32(loc.as_ref(), 0);
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
        }
    }
}

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
        panic!("Shader compilation failed: {log}");
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
        panic!("Shader linking failed: {log}");
    }
    program
}
