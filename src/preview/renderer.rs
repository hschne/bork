//! OpenGL interface adaptation only. All interference lives in signal.glsl.
use crate::{
    artwork::Canvas,
    effect::{Glitchiness, Settings},
};
use anyhow::{Result, bail, ensure};
use eframe::{
    egui_glow::ShaderVersion,
    glow::{self, HasContext},
};

pub struct Renderer {
    program: glow::Program,
    vao: glow::VertexArray,
    texture: glow::Texture,
    body: String,
}

impl Renderer {
    pub fn new(gl: &glow::Context, settings: &Settings, level: Glitchiness) -> Result<Self> {
        let body = settings.shader_body(level);
        let program = compile(gl, &body)?;
        // SAFETY: eframe's creation callback has made this GL context current. The
        // handles are kept with this renderer and deleted only in the exit callback.
        unsafe {
            let vao = match gl.create_vertex_array() {
                Ok(vao) => vao,
                Err(error) => {
                    gl.delete_program(program);
                    bail!(error);
                }
            };
            let texture = match gl.create_texture() {
                Ok(texture) => texture,
                Err(error) => {
                    gl.delete_vertex_array(vao);
                    gl.delete_program(program);
                    bail!(error);
                }
            };
            Ok(Self {
                program,
                vao,
                texture,
                body,
            })
        }
    }

    pub fn update(
        &mut self,
        gl: &glow::Context,
        settings: &Settings,
        level: Glitchiness,
    ) -> Result<()> {
        let body = settings.shader_body(level);
        if self.body != body {
            let program = compile(gl, &body)?;
            // SAFETY: called exclusively during a current-context paint callback.
            unsafe {
                gl.delete_program(self.program);
            }
            self.program = program;
            self.body = body;
        }
        Ok(())
    }

    pub fn paint(&self, gl: &glow::Context, canvas: &Canvas, time: f32, seed: u32) {
        // SAFETY: called only from egui's GL paint callback, on the context's thread.
        // Canvas owns width*height*4 initialized bytes; no user-supplied raw pointers.
        // gl_VertexID needs no vertex buffers. All used handles belong to this context.
        unsafe {
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                canvas.width() as i32,
                canvas.height() as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(canvas.rgba())),
            );
            gl.uniform_1_i32(gl.get_uniform_location(self.program, "tex").as_ref(), 0);
            gl.uniform_1_f32(gl.get_uniform_location(self.program, "time").as_ref(), time);
            gl.uniform_1_u32(
                gl.get_uniform_location(self.program, "run_seed").as_ref(),
                seed,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_vertex_array(None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.use_program(None);
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        // SAFETY: eframe calls on_exit with the same current GL context, once.
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_texture(self.texture);
        }
    }
}

fn compile(gl: &glow::Context, body: &str) -> Result<glow::Program> {
    let version = ShaderVersion::get(gl);
    ensure!(
        version.is_new_shader_interface(),
        "Preview requires desktop OpenGL 3+ or GLES 3+"
    );
    let header = format!(
        "{}{}",
        version.version_declaration(),
        if version.is_embedded() {
            "precision highp float;\nprecision highp int;\n"
        } else {
            ""
        }
    );
    let vertex = format!(
        "{header}out vec2 uv;\nvoid main() {{ vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2)); uv = vec2(p.x, 1.0-p.y); gl_Position = vec4(p*2.0-1.0, 0.0, 1.0); }}"
    );
    let fragment = format!(
        "{header}in vec2 uv; out vec4 out_colour; uniform sampler2D tex; uniform float time; uniform uint run_seed;\nvec4 sampleDesktop(vec2 p) {{ return texture(tex, clamp(p, 0.0, 1.0)); }}\n{body}\nvoid main() {{ out_colour = signalLoss(uv, time, run_seed); }}"
    );
    // SAFETY: invoked only in current-context creation/paint callbacks. Shader source
    // lifetimes cover each upload. All temporary shader handles are deleted on every path.
    unsafe {
        let program = gl.create_program().map_err(anyhow::Error::msg)?;
        for (kind, source) in [
            (glow::VERTEX_SHADER, vertex),
            (glow::FRAGMENT_SHADER, fragment),
        ] {
            let shader = match gl.create_shader(kind) {
                Ok(shader) => shader,
                Err(error) => {
                    gl.delete_program(program);
                    bail!(error);
                }
            };
            gl.shader_source(shader, &source);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let error = gl.get_shader_info_log(shader);
                gl.delete_shader(shader);
                gl.delete_program(program);
                bail!("Shader compilation failed: {error}");
            }
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            let error = gl.get_program_info_log(program);
            gl.delete_program(program);
            bail!("Shader link failed: {error}");
        }
        Ok(program)
    }
}
