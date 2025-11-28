use std::collections::HashMap;
use std::os::fd::{OwnedFd, BorrowedFd, AsFd};
use std::rc::Rc;

use drm::control::Device as ControlDevice;
use gbm::{AsRaw, BufferObjectFlags, Device as GbmDevice};
use glow::HasContext;
use khronos_egl as egl;

pub type GlowContext = glow::Context;

const EGL_PLATFORM_GBM_KHR: egl::Enum = 0x31D7;

// EGL_EXT_image_dma_buf_import (for future DMA-BUF texture import)
#[allow(dead_code)]
const EGL_LINUX_DMA_BUF_EXT: egl::Enum = 0x3270;
#[allow(dead_code)]
const EGL_LINUX_DRM_FOURCC_EXT: egl::Int = 0x3271;
#[allow(dead_code)]
const EGL_DMA_BUF_PLANE0_FD_EXT: egl::Int = 0x3272;
#[allow(dead_code)]
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: egl::Int = 0x3273;
#[allow(dead_code)]
const EGL_DMA_BUF_PLANE0_PITCH_EXT: egl::Int = 0x3274;
#[allow(dead_code)]
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: egl::Int = 0x3443;
#[allow(dead_code)]
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: egl::Int = 0x3444;

const VERTEX_SHADER: &str = r#"#version 100
attribute vec2 a_position;
attribute vec2 a_texcoord;
varying vec2 v_texcoord;

uniform vec2 u_offset;
uniform vec2 u_size;
uniform vec2 u_screen_size;

void main() {
    vec2 pos = (a_position * u_size + u_offset) / u_screen_size * 2.0 - 1.0;
    pos.y = -pos.y;
    gl_Position = vec4(pos, 0.0, 1.0);
    v_texcoord = a_texcoord;
}
"#;

const FRAGMENT_SHADER_TEXTURE: &str = r#"#version 100
precision mediump float;
varying vec2 v_texcoord;
uniform sampler2D u_texture;

void main() {
    gl_FragColor = texture2D(u_texture, v_texcoord);
}
"#;

const FRAGMENT_SHADER_COLOR: &str = r#"#version 100
precision mediump float;
uniform vec4 u_color;

void main() {
    gl_FragColor = u_color;
}
"#;

struct DrmCard(std::fs::File);

impl AsFd for DrmCard {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for DrmCard {}
impl ControlDevice for DrmCard {}

pub struct GpuRenderer {
    egl: Rc<egl::DynamicInstance<egl::EGL1_5>>,
    display: egl::Display,
    context: egl::Context,
    surface: egl::Surface,
    gl: Rc<GlowContext>,
    
    #[allow(dead_code)]
    gbm: GbmDevice<std::fs::File>,
    
    texture_program: glow::Program,
    color_program: glow::Program,
    quad_vao: glow::VertexArray,
    #[allow(dead_code)]
    quad_vbo: glow::Buffer,
    
    width: u32,
    height: u32,
    
    shm_textures: HashMap<u64, glow::Texture>,
    dmabuf_textures: HashMap<u64, (glow::Texture, egl::Image)>,
    
    pub supported_formats: Vec<DmaBufFormat>,
}

#[derive(Clone, Debug)]
pub struct DmaBufFormat {
    pub format: u32,
    pub modifier: u64,
}

#[allow(dead_code)]
pub struct DmaBufInfo {
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub stride: u32,
    pub offset: u32,
    pub modifier: u64,
}

impl GpuRenderer {
    pub fn new(drm_device: std::fs::File) -> Result<Self, Box<dyn std::error::Error>> {
        let gbm = GbmDevice::new(drm_device.try_clone()?)?;
        
        let egl = Rc::new(unsafe { 
            egl::DynamicInstance::<egl::EGL1_5>::load_required()
                .map_err(|e| format!("Failed to load EGL: {:?}", e))?
        });
        
        let card = DrmCard(drm_device.try_clone()?);
        let res = card.resource_handles()?;
        let connectors: Vec<_> = res.connectors().iter()
            .filter_map(|&conn| card.get_connector(conn, true).ok())
            .collect();
        
        let connector_info = connectors.iter()
            .find(|c| c.state() == drm::control::connector::State::Connected)
            .ok_or("No connected display found")?;
        
        let mode = connector_info.modes().first()
            .ok_or("No display mode available")?;
        
        let (width, height) = mode.size();
        let width = width as u32;
        let height = height as u32;
        
        log::info!("[gpu] Display mode: {}x{}", width, height);
        
        let gbm_ptr = gbm.as_raw() as *mut std::ffi::c_void;
        let display = unsafe {
            egl.get_platform_display(EGL_PLATFORM_GBM_KHR, gbm_ptr, &[egl::NONE as egl::Attrib])
                .map_err(|e| format!("Failed to get EGL display: {:?}", e))?
        };
        
        egl.initialize(display)?;
        
        let extensions = egl.query_string(Some(display), egl::EXTENSIONS)
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        
        log::info!("[gpu] EGL extensions available");
        
        let has_dmabuf_import = extensions.contains("EGL_EXT_image_dma_buf_import");
        log::info!("[gpu] DMA-BUF import: {}", has_dmabuf_import);
        
        let config_attribs = [
            egl::SURFACE_TYPE, egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE, egl::OPENGL_ES2_BIT,
            egl::RED_SIZE, 8,
            egl::GREEN_SIZE, 8,
            egl::BLUE_SIZE, 8,
            egl::ALPHA_SIZE, 8,
            egl::NONE,
        ];
        
        let config = egl.choose_first_config(display, &config_attribs)?
            .ok_or("No suitable EGL config")?;
        
        let gbm_surface = gbm.create_surface::<()>(
            width,
            height,
            gbm::Format::Xrgb8888,
            BufferObjectFlags::SCANOUT | BufferObjectFlags::RENDERING,
        )?;
        
        let gbm_surface_ptr = gbm_surface.as_raw() as *mut std::ffi::c_void;
        let surface = unsafe {
            egl.create_platform_window_surface(
                display,
                config,
                gbm_surface_ptr,
                &[egl::NONE as egl::Attrib],
            )?
        };
        
        egl.bind_api(egl::OPENGL_ES_API)?;
        
        let context_attribs = [
            egl::CONTEXT_CLIENT_VERSION, 2,
            egl::NONE,
        ];
        
        let context = egl.create_context(display, config, None, &context_attribs)?;
        
        egl.make_current(display, Some(surface), Some(surface), Some(context))?;
        
        let gl = Rc::new(unsafe {
            GlowContext::from_loader_function(|s| {
                egl.get_proc_address(s)
                    .map(|p| p as *const _)
                    .unwrap_or(std::ptr::null())
            })
        });
        
        log::info!("[gpu] OpenGL version: {:?}", unsafe { gl.get_parameter_string(glow::VERSION) });
        log::info!("[gpu] OpenGL renderer: {:?}", unsafe { gl.get_parameter_string(glow::RENDERER) });
        let texture_program = Self::create_program(&gl, VERTEX_SHADER, FRAGMENT_SHADER_TEXTURE)?;
        let color_program = Self::create_program(&gl, VERTEX_SHADER, FRAGMENT_SHADER_COLOR)?;
        let (quad_vao, quad_vbo) = Self::create_quad(&gl)?;
        let supported_formats = Self::query_dmabuf_formats();
        log::info!("[gpu] Supported DMA-BUF formats: {}", supported_formats.len());
        std::mem::forget(gbm_surface);
        
        Ok(Self {
            egl,
            display,
            context,
            surface,
            gl,
            gbm,
            texture_program,
            color_program,
            quad_vao,
            quad_vbo,
            width,
            height,
            shm_textures: HashMap::new(),
            dmabuf_textures: HashMap::new(),
            supported_formats,
        })
    }
    
    fn create_program(gl: &GlowContext, vs_src: &str, fs_src: &str) -> Result<glow::Program, Box<dyn std::error::Error>> {
        unsafe {
            let program = gl.create_program()?;
            
            let vs = gl.create_shader(glow::VERTEX_SHADER)?;
            gl.shader_source(vs, vs_src);
            gl.compile_shader(vs);
            if !gl.get_shader_compile_status(vs) {
                let log = gl.get_shader_info_log(vs);
                return Err(format!("Vertex shader error: {}", log).into());
            }
            
            let fs = gl.create_shader(glow::FRAGMENT_SHADER)?;
            gl.shader_source(fs, fs_src);
            gl.compile_shader(fs);
            if !gl.get_shader_compile_status(fs) {
                let log = gl.get_shader_info_log(fs);
                return Err(format!("Fragment shader error: {}", log).into());
            }
            
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            
            gl.bind_attrib_location(program, 0, "a_position");
            gl.bind_attrib_location(program, 1, "a_texcoord");
            
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                return Err(format!("Program link error: {}", log).into());
            }
            
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            
            Ok(program)
        }
    }
    
    fn create_quad(gl: &GlowContext) -> Result<(glow::VertexArray, glow::Buffer), Box<dyn std::error::Error>> {
        unsafe {
            let vao = gl.create_vertex_array()?;
            gl.bind_vertex_array(Some(vao));
            
            let vbo = gl.create_buffer()?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            
            // Position (x, y) + Texcoord (u, v)
            #[rustfmt::skip]
            let vertices: [f32; 24] = [
                // pos      // tex
                0.0, 0.0,   0.0, 0.0,
                1.0, 0.0,   1.0, 0.0,
                0.0, 1.0,   0.0, 1.0,
                1.0, 0.0,   1.0, 0.0,
                1.0, 1.0,   1.0, 1.0,
                0.0, 1.0,   0.0, 1.0,
            ];
            
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck_cast_slice(&vertices),
                glow::STATIC_DRAW,
            );
            
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
            
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);
            
            gl.bind_vertex_array(None);
            
            Ok((vao, vbo))
        }
    }
    
    fn query_dmabuf_formats() -> Vec<DmaBufFormat> {
        vec![
            DmaBufFormat { format: drm_fourcc::DrmFourcc::Argb8888 as u32, modifier: drm_fourcc::DrmModifier::Linear.into() },
            DmaBufFormat { format: drm_fourcc::DrmFourcc::Xrgb8888 as u32, modifier: drm_fourcc::DrmModifier::Linear.into() },
            DmaBufFormat { format: drm_fourcc::DrmFourcc::Abgr8888 as u32, modifier: drm_fourcc::DrmModifier::Linear.into() },
            DmaBufFormat { format: drm_fourcc::DrmFourcc::Xbgr8888 as u32, modifier: drm_fourcc::DrmModifier::Linear.into() },
        ]
    }
    
    pub fn begin_frame(&mut self) {
        self.egl.make_current(self.display, Some(self.surface), Some(self.surface), Some(self.context)).ok();
        
        unsafe {
            self.gl.viewport(0, 0, self.width as i32, self.height as i32);
            self.gl.clear_color(0.1, 0.1, 0.12, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }
    
    pub fn end_frame(&mut self) {
        self.egl.swap_buffers(self.display, self.surface).ok();
    }
    
    pub fn draw_rect(&self, x: i32, y: i32, width: i32, height: i32, color: [f32; 4]) {
        unsafe {
            self.gl.use_program(Some(self.color_program));
            
            let offset_loc = self.gl.get_uniform_location(self.color_program, "u_offset");
            let size_loc = self.gl.get_uniform_location(self.color_program, "u_size");
            let screen_loc = self.gl.get_uniform_location(self.color_program, "u_screen_size");
            let color_loc = self.gl.get_uniform_location(self.color_program, "u_color");
            
            self.gl.uniform_2_f32(offset_loc.as_ref(), x as f32, y as f32);
            self.gl.uniform_2_f32(size_loc.as_ref(), width as f32, height as f32);
            self.gl.uniform_2_f32(screen_loc.as_ref(), self.width as f32, self.height as f32);
            self.gl.uniform_4_f32(color_loc.as_ref(), color[0], color[1], color[2], color[3]);
            
            self.gl.bind_vertex_array(Some(self.quad_vao));
            self.gl.draw_arrays(glow::TRIANGLES, 0, 6);
        }
    }
    
    pub fn upload_shm_texture(&mut self, id: u64, width: u32, height: u32, data: &[u8]) -> glow::Texture {
        unsafe {
            let texture = if let Some(&tex) = self.shm_textures.get(&id) {
                tex
            } else {
                let tex = self.gl.create_texture().unwrap();
                self.shm_textures.insert(id, tex);
                tex
            };
            
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            self.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            self.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            self.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            self.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            
            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                width as i32,
                height as i32,
                0,
                glow::BGRA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(data)),
            );
            
            texture
        }
    }
    
    pub fn draw_texture(&self, texture: glow::Texture, x: i32, y: i32, width: i32, height: i32) {
        unsafe {
            self.gl.use_program(Some(self.texture_program));
            
            let offset_loc = self.gl.get_uniform_location(self.texture_program, "u_offset");
            let size_loc = self.gl.get_uniform_location(self.texture_program, "u_size");
            let screen_loc = self.gl.get_uniform_location(self.texture_program, "u_screen_size");
            let tex_loc = self.gl.get_uniform_location(self.texture_program, "u_texture");
            
            self.gl.uniform_2_f32(offset_loc.as_ref(), x as f32, y as f32);
            self.gl.uniform_2_f32(size_loc.as_ref(), width as f32, height as f32);
            self.gl.uniform_2_f32(screen_loc.as_ref(), self.width as f32, self.height as f32);
            self.gl.uniform_1_i32(tex_loc.as_ref(), 0);
            
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            
            self.gl.bind_vertex_array(Some(self.quad_vao));
            self.gl.draw_arrays(glow::TRIANGLES, 0, 6);
        }
    }
    
    pub fn remove_texture(&mut self, id: u64) {
        if let Some(tex) = self.shm_textures.remove(&id) {
            unsafe { self.gl.delete_texture(tex); }
        }
        if let Some((tex, img)) = self.dmabuf_textures.remove(&id) {
            unsafe {
                self.gl.delete_texture(tex);
                self.egl.destroy_image(self.display, img).ok();
            }
        }
    }
    
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

impl Drop for GpuRenderer {
    fn drop(&mut self) {
        for (_, tex) in self.shm_textures.drain() {
            unsafe { self.gl.delete_texture(tex); }
        }
        for (_, (tex, img)) in self.dmabuf_textures.drain() {
            unsafe {
                self.gl.delete_texture(tex);
                self.egl.destroy_image(self.display, img).ok();
            }
        }
        
        unsafe {
            self.gl.delete_program(self.texture_program);
            self.gl.delete_program(self.color_program);
            self.gl.delete_vertex_array(self.quad_vao);
            self.gl.delete_buffer(self.quad_vbo);
        }
        
        self.egl.destroy_surface(self.display, self.surface).ok();
        self.egl.destroy_context(self.display, self.context).ok();
        self.egl.terminate(self.display).ok();
    }
}

fn bytemuck_cast_slice(data: &[f32]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            data.as_ptr() as *const u8,
            std::mem::size_of_val(data),
        )
    }
}
