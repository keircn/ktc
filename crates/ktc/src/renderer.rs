use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::rc::Rc;

use drm::control::{connector, crtc, framebuffer, Device as ControlDevice};
use gbm::{AsRaw, BufferObjectFlags, Device as GbmDevice};
use glow::HasContext;
use khronos_egl as egl;

pub type GlowContext = glow::Context;

const EGL_PLATFORM_GBM_KHR: egl::Enum = 0x31D7;
const EGL_LINUX_DMA_BUF_EXT: egl::Enum = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: egl::Int = 0x3271;

const EGL_DMA_BUF_PLANE0_FD_EXT: egl::Int = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: egl::Int = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: egl::Int = 0x3274;
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: egl::Int = 0x3443;
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: egl::Int = 0x3444;

const EGL_DMA_BUF_PLANE1_FD_EXT: egl::Int = 0x3275;
const EGL_DMA_BUF_PLANE1_OFFSET_EXT: egl::Int = 0x3276;
const EGL_DMA_BUF_PLANE1_PITCH_EXT: egl::Int = 0x3277;
const EGL_DMA_BUF_PLANE1_MODIFIER_LO_EXT: egl::Int = 0x3445;
const EGL_DMA_BUF_PLANE1_MODIFIER_HI_EXT: egl::Int = 0x3446;

const EGL_DMA_BUF_PLANE2_FD_EXT: egl::Int = 0x3278;
const EGL_DMA_BUF_PLANE2_OFFSET_EXT: egl::Int = 0x3279;
const EGL_DMA_BUF_PLANE2_PITCH_EXT: egl::Int = 0x327A;
const EGL_DMA_BUF_PLANE2_MODIFIER_LO_EXT: egl::Int = 0x3447;
const EGL_DMA_BUF_PLANE2_MODIFIER_HI_EXT: egl::Int = 0x3448;

const EGL_DMA_BUF_PLANE3_FD_EXT: egl::Int = 0x3440;
const EGL_DMA_BUF_PLANE3_OFFSET_EXT: egl::Int = 0x3441;
const EGL_DMA_BUF_PLANE3_PITCH_EXT: egl::Int = 0x3442;
const EGL_DMA_BUF_PLANE3_MODIFIER_LO_EXT: egl::Int = 0x3449;
const EGL_DMA_BUF_PLANE3_MODIFIER_HI_EXT: egl::Int = 0x344A;

#[allow(dead_code)]
const GL_TEXTURE_EXTERNAL_OES: u32 = 0x8D65;

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
    vec4 color = texture2D(u_texture, v_texcoord);
    gl_FragColor = vec4(color.rgb, 1.0);
}
"#;

const FRAGMENT_SHADER_EXTERNAL: &str = r#"#version 100
#extension GL_OES_EGL_image_external : require
precision mediump float;
varying vec2 v_texcoord;
uniform samplerExternalOES u_texture;

void main() {
    vec4 color = texture2D(u_texture, v_texcoord);
    gl_FragColor = vec4(color.rgb, 1.0);
}
"#;

const FRAGMENT_SHADER_DMABUF: &str = r#"#version 100
precision mediump float;
varying vec2 v_texcoord;
uniform sampler2D u_texture;

void main() {
    vec4 color = texture2D(u_texture, v_texcoord);
    gl_FragColor = vec4(color.rgb, 1.0);
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
    gbm_surface: *mut std::ffi::c_void,
    drm_card: DrmCard,
    crtc: crtc::Handle,
    connector: connector::Handle,
    mode: drm::control::Mode,

    current_fb: Option<framebuffer::Handle>,
    current_bo: *mut gbm_sys::gbm_bo,
    next_fb: Option<framebuffer::Handle>,
    next_bo: *mut gbm_sys::gbm_bo,
    mode_set: bool,
    flip_pending: bool,

    texture_program: glow::Program,
    external_program: Option<glow::Program>,
    dmabuf_program: glow::Program,
    color_program: glow::Program,
    quad_vao: glow::VertexArray,
    #[allow(dead_code)]
    quad_vbo: glow::Buffer,

    width: u32,
    height: u32,
    physical_width: u32,
    physical_height: u32,

    shm_textures: HashMap<u64, glow::Texture>,
    dmabuf_textures: HashMap<u64, (glow::Texture, egl::Image, bool)>,

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
    #[allow(dead_code)]
    pub fn new(drm_device: std::fs::File) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_with_config(drm_device, None, true)
    }

    pub fn new_with_config(
        drm_device: std::fs::File,
        preferred_mode: Option<(u16, u16, Option<u32>)>,
        _vsync: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let gbm = GbmDevice::new(drm_device.try_clone()?)?;

        let egl = Rc::new(unsafe {
            egl::DynamicInstance::<egl::EGL1_5>::load_required()
                .map_err(|e| format!("Failed to load EGL: {:?}", e))?
        });

        let card = DrmCard(drm_device.try_clone()?);
        let res = card.resource_handles()?;
        let connectors: Vec<_> = res
            .connectors()
            .iter()
            .filter_map(|&conn| card.get_connector(conn, true).ok())
            .collect();

        let connector_info = connectors
            .iter()
            .find(|c| c.state() == drm::control::connector::State::Connected)
            .ok_or("No connected display found")?;

        let connector_handle = connector_info.handle();

        log::info!("[gpu] Available display modes:");
        for m in connector_info.modes() {
            let (w, h) = m.size();
            log::info!("[gpu]   {}x{}@{}Hz", w, h, m.vrefresh());
        }

        let mode = if let Some((pref_w, pref_h, pref_refresh)) = preferred_mode {
            connector_info
                .modes()
                .iter()
                .find(|m| {
                    let (w, h) = m.size();
                    let matches_res = w == pref_w && h == pref_h;
                    if let Some(refresh) = pref_refresh {
                        matches_res && m.vrefresh() == refresh
                    } else {
                        matches_res
                    }
                })
                .or_else(|| {
                    log::warn!(
                        "[gpu] Preferred mode {}x{}{} not found, using default",
                        pref_w,
                        pref_h,
                        pref_refresh
                            .map(|r| format!("@{}Hz", r))
                            .unwrap_or_default()
                    );
                    connector_info.modes().first()
                })
                .copied()
                .ok_or("No display mode available")?
        } else {
            *connector_info
                .modes()
                .first()
                .ok_or("No display mode available")?
        };

        let (width, height) = mode.size();
        let width = width as u32;
        let height = height as u32;

        let (physical_width, physical_height) = connector_info.size().unwrap_or((0, 0));
        log::info!(
            "[gpu] Physical size: {}x{}mm",
            physical_width,
            physical_height
        );

        let crtc_handle = res.crtcs().first().copied().ok_or("No CRTC available")?;

        log::info!(
            "[gpu] Selected mode: {}x{}@{}Hz",
            width,
            height,
            mode.vrefresh()
        );

        let gbm_ptr = gbm.as_raw() as *mut std::ffi::c_void;
        let display = unsafe {
            egl.get_platform_display(EGL_PLATFORM_GBM_KHR, gbm_ptr, &[egl::NONE as egl::Attrib])
                .map_err(|e| format!("Failed to get EGL display: {:?}", e))?
        };

        egl.initialize(display)?;

        let extensions = egl
            .query_string(Some(display), egl::EXTENSIONS)
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        log::info!("[gpu] EGL extensions available");

        let has_dmabuf_import = extensions.contains("EGL_EXT_image_dma_buf_import");
        log::info!("[gpu] DMA-BUF import: {}", has_dmabuf_import);

        let config_attribs = [
            egl::SURFACE_TYPE,
            egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_ES2_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            8,
            egl::NONE,
        ];

        let config = egl
            .choose_first_config(display, &config_attribs)?
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

        let context_attribs = [egl::CONTEXT_CLIENT_VERSION, 2, egl::NONE];

        let context = egl.create_context(display, config, None, &context_attribs)?;

        egl.make_current(display, Some(surface), Some(surface), Some(context))?;

        let gl = Rc::new(unsafe {
            GlowContext::from_loader_function(|s| {
                egl.get_proc_address(s)
                    .map(|p| p as *const _)
                    .unwrap_or(std::ptr::null())
            })
        });

        log::info!("[gpu] OpenGL version: {:?}", unsafe {
            gl.get_parameter_string(glow::VERSION)
        });
        log::info!("[gpu] OpenGL renderer: {:?}", unsafe {
            gl.get_parameter_string(glow::RENDERER)
        });
        let texture_program = Self::create_program(&gl, VERTEX_SHADER, FRAGMENT_SHADER_TEXTURE)?;
        let external_program =
            Self::create_program(&gl, VERTEX_SHADER, FRAGMENT_SHADER_EXTERNAL).ok();
        if external_program.is_some() {
            log::info!("[gpu] GL_OES_EGL_image_external supported");
        } else {
            log::warn!("[gpu] GL_OES_EGL_image_external not supported, DMA-BUF may not work");
        }
        let dmabuf_program = Self::create_program(&gl, VERTEX_SHADER, FRAGMENT_SHADER_DMABUF)?;
        let color_program = Self::create_program(&gl, VERTEX_SHADER, FRAGMENT_SHADER_COLOR)?;
        let (quad_vao, quad_vbo) = Self::create_quad(&gl)?;
        let supported_formats = Self::query_dmabuf_formats(&egl, display);
        log::info!(
            "[gpu] Supported DMA-BUF formats: {}",
            supported_formats.len()
        );
        let gbm_surface_raw = gbm_surface.as_raw() as *mut std::ffi::c_void;
        std::mem::forget(gbm_surface);

        Ok(Self {
            egl,
            display,
            context,
            surface,
            gl,
            gbm,
            gbm_surface: gbm_surface_raw,
            drm_card: card,
            crtc: crtc_handle,
            connector: connector_handle,
            mode,
            current_fb: None,
            current_bo: std::ptr::null_mut(),
            next_fb: None,
            next_bo: std::ptr::null_mut(),
            mode_set: false,
            flip_pending: false,
            texture_program,
            external_program,
            dmabuf_program,
            color_program,
            quad_vao,
            quad_vbo,
            width,
            height,
            physical_width,
            physical_height,
            shm_textures: HashMap::new(),
            dmabuf_textures: HashMap::new(),
            supported_formats,
        })
    }

    fn create_program(
        gl: &GlowContext,
        vs_src: &str,
        fs_src: &str,
    ) -> Result<glow::Program, Box<dyn std::error::Error>> {
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

    fn create_quad(
        gl: &GlowContext,
    ) -> Result<(glow::VertexArray, glow::Buffer), Box<dyn std::error::Error>> {
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

    fn query_dmabuf_formats(
        egl: &egl::DynamicInstance<egl::EGL1_5>,
        display: egl::Display,
    ) -> Vec<DmaBufFormat> {
        let mut formats = Vec::new();

        #[allow(improper_ctypes_definitions)]
        type QueryDmaBufFormatsEXT =
            unsafe extern "C" fn(*const std::ffi::c_void, i32, *mut i32, *mut i32) -> u32;

        #[allow(improper_ctypes_definitions)]
        type QueryDmaBufModifiersEXT = unsafe extern "C" fn(
            *const std::ffi::c_void,
            i32,
            i32,
            *mut u64,
            *mut u32,
            *mut i32,
        ) -> u32;

        let query_formats: Option<QueryDmaBufFormatsEXT> = egl
            .get_proc_address("eglQueryDmaBufFormatsEXT")
            .map(|p| unsafe { std::mem::transmute(p) });
        let query_modifiers: Option<QueryDmaBufModifiersEXT> = egl
            .get_proc_address("eglQueryDmaBufModifiersEXT")
            .map(|p| unsafe { std::mem::transmute(p) });

        let (query_formats, query_modifiers) = match (query_formats, query_modifiers) {
            (Some(f), Some(m)) => (f, m),
            _ => {
                log::warn!("[gpu] EGL_EXT_image_dma_buf_import_modifiers not available, using fallback formats");
                return Self::fallback_dmabuf_formats();
            }
        };

        let display_ptr = display.as_ptr();

        let mut num_formats: i32 = 0;
        let result =
            unsafe { query_formats(display_ptr, 0, std::ptr::null_mut(), &mut num_formats) };
        if result == 0 || num_formats <= 0 {
            log::warn!("[gpu] eglQueryDmaBufFormatsEXT failed, using fallback formats");
            return Self::fallback_dmabuf_formats();
        }

        let mut format_list = vec![0i32; num_formats as usize];
        let result = unsafe {
            query_formats(
                display_ptr,
                num_formats,
                format_list.as_mut_ptr(),
                &mut num_formats,
            )
        };
        if result == 0 {
            log::warn!("[gpu] eglQueryDmaBufFormatsEXT (get) failed, using fallback formats");
            return Self::fallback_dmabuf_formats();
        }

        log::info!("[gpu] EGL reports {} DMA-BUF formats", num_formats);

        for &format in &format_list {
            let mut num_modifiers: i32 = 0;
            let result = unsafe {
                query_modifiers(
                    display_ptr,
                    format,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut num_modifiers,
                )
            };

            if result == 0 || num_modifiers <= 0 {
                formats.push(DmaBufFormat {
                    format: format as u32,
                    modifier: drm_fourcc::DrmModifier::Invalid.into(),
                });
                continue;
            }

            let mut modifiers = vec![0u64; num_modifiers as usize];
            let mut external_only = vec![0u32; num_modifiers as usize];
            let result = unsafe {
                query_modifiers(
                    display_ptr,
                    format,
                    num_modifiers,
                    modifiers.as_mut_ptr(),
                    external_only.as_mut_ptr(),
                    &mut num_modifiers,
                )
            };

            if result == 0 {
                formats.push(DmaBufFormat {
                    format: format as u32,
                    modifier: drm_fourcc::DrmModifier::Invalid.into(),
                });
                continue;
            }

            for (i, &modifier) in modifiers.iter().enumerate() {
                if external_only.get(i).copied().unwrap_or(0) == 0 {
                    formats.push(DmaBufFormat {
                        format: format as u32,
                        modifier,
                    });
                }
            }
        }

        if formats.is_empty() {
            log::warn!("[gpu] No DMA-BUF formats found via EGL, using fallback");
            return Self::fallback_dmabuf_formats();
        }

        formats
    }

    fn fallback_dmabuf_formats() -> Vec<DmaBufFormat> {
        vec![
            DmaBufFormat {
                format: drm_fourcc::DrmFourcc::Argb8888 as u32,
                modifier: drm_fourcc::DrmModifier::Invalid.into(),
            },
            DmaBufFormat {
                format: drm_fourcc::DrmFourcc::Xrgb8888 as u32,
                modifier: drm_fourcc::DrmModifier::Invalid.into(),
            },
            DmaBufFormat {
                format: drm_fourcc::DrmFourcc::Abgr8888 as u32,
                modifier: drm_fourcc::DrmModifier::Invalid.into(),
            },
            DmaBufFormat {
                format: drm_fourcc::DrmFourcc::Xbgr8888 as u32,
                modifier: drm_fourcc::DrmModifier::Invalid.into(),
            },
            DmaBufFormat {
                format: drm_fourcc::DrmFourcc::Argb8888 as u32,
                modifier: drm_fourcc::DrmModifier::Linear.into(),
            },
            DmaBufFormat {
                format: drm_fourcc::DrmFourcc::Xrgb8888 as u32,
                modifier: drm_fourcc::DrmModifier::Linear.into(),
            },
            DmaBufFormat {
                format: drm_fourcc::DrmFourcc::Abgr8888 as u32,
                modifier: drm_fourcc::DrmModifier::Linear.into(),
            },
            DmaBufFormat {
                format: drm_fourcc::DrmFourcc::Xbgr8888 as u32,
                modifier: drm_fourcc::DrmModifier::Linear.into(),
            },
        ]
    }

    pub fn begin_frame(&mut self) {
        self.egl
            .make_current(
                self.display,
                Some(self.surface),
                Some(self.surface),
                Some(self.context),
            )
            .ok();

        unsafe {
            self.gl
                .viewport(0, 0, self.width as i32, self.height as i32);
            self.gl.clear_color(0.1, 0.1, 0.12, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }

    pub fn end_frame(&mut self) {
        if self.flip_pending {
            self.wait_for_flip();
            self.flip_pending = false;

            unsafe {
                let gbm_surface = self.gbm_surface as *mut gbm_sys::gbm_surface;
                if let Some(old_fb) = self.current_fb.take() {
                    self.drm_card.destroy_framebuffer(old_fb).ok();
                }
                if !self.current_bo.is_null() {
                    gbm_sys::gbm_surface_release_buffer(gbm_surface, self.current_bo);
                }
                self.current_fb = self.next_fb.take();
                self.current_bo = self.next_bo;
                self.next_bo = std::ptr::null_mut();
            }
        }

        self.egl.swap_buffers(self.display, self.surface).ok();

        unsafe {
            let gbm_surface = self.gbm_surface as *mut gbm_sys::gbm_surface;
            let bo = gbm_sys::gbm_surface_lock_front_buffer(gbm_surface);
            if bo.is_null() {
                log::error!("[gpu] Failed to lock front buffer");
                return;
            }

            let handle = gbm_sys::gbm_bo_get_handle(bo).u32_;
            let stride = gbm_sys::gbm_bo_get_stride(bo);
            let width = gbm_sys::gbm_bo_get_width(bo);
            let height = gbm_sys::gbm_bo_get_height(bo);

            struct GbmBuffer {
                handle: u32,
                width: u32,
                height: u32,
                stride: u32,
            }

            impl drm::buffer::Buffer for GbmBuffer {
                fn size(&self) -> (u32, u32) {
                    (self.width, self.height)
                }
                fn format(&self) -> drm::buffer::DrmFourcc {
                    drm::buffer::DrmFourcc::Xrgb8888
                }
                fn pitch(&self) -> u32 {
                    self.stride
                }
                fn handle(&self) -> drm::buffer::Handle {
                    drm::buffer::Handle::from(std::num::NonZeroU32::new(self.handle).unwrap())
                }
            }

            let buffer = GbmBuffer {
                handle,
                width,
                height,
                stride,
            };

            let fb = match self.drm_card.add_framebuffer(&buffer, 24, 32) {
                Ok(fb) => fb,
                Err(e) => {
                    log::error!("[gpu] Failed to add framebuffer: {}", e);
                    gbm_sys::gbm_surface_release_buffer(gbm_surface, bo);
                    return;
                }
            };

            if !self.mode_set {
                if let Err(e) = self.drm_card.set_crtc(
                    self.crtc,
                    Some(fb),
                    (0, 0),
                    &[self.connector],
                    Some(self.mode),
                ) {
                    log::error!("[gpu] set_crtc failed: {}", e);
                    self.drm_card.destroy_framebuffer(fb).ok();
                    gbm_sys::gbm_surface_release_buffer(gbm_surface, bo);
                    return;
                }
                self.mode_set = true;
                self.current_fb = Some(fb);
                self.current_bo = bo;
            } else {
                use drm::control::PageFlipFlags;

                match self
                    .drm_card
                    .page_flip(self.crtc, fb, PageFlipFlags::EVENT, None)
                {
                    Ok(()) => {
                        self.next_fb = Some(fb);
                        self.next_bo = bo;
                        self.flip_pending = true;
                    }
                    Err(e) => {
                        log::warn!("[gpu] page_flip failed: {}, falling back to set_crtc", e);
                        if let Err(e) = self.drm_card.set_crtc(
                            self.crtc,
                            Some(fb),
                            (0, 0),
                            &[self.connector],
                            Some(self.mode),
                        ) {
                            log::error!("[gpu] set_crtc fallback failed: {}", e);
                            self.drm_card.destroy_framebuffer(fb).ok();
                            gbm_sys::gbm_surface_release_buffer(gbm_surface, bo);
                            return;
                        }

                        if let Some(old_fb) = self.current_fb.take() {
                            self.drm_card.destroy_framebuffer(old_fb).ok();
                        }
                        if !self.current_bo.is_null() {
                            gbm_sys::gbm_surface_release_buffer(gbm_surface, self.current_bo);
                        }

                        self.current_fb = Some(fb);
                        self.current_bo = bo;
                    }
                }
            }
        }
    }

    fn wait_for_flip(&self) {
        use std::os::fd::AsRawFd;

        let fd = self.drm_card.as_fd().as_raw_fd();
        let mut fds = [libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        }];

        let timeout_ms = 16;

        unsafe {
            let ret = libc::poll(fds.as_mut_ptr(), 1, timeout_ms);
            if ret > 0 && (fds[0].revents & libc::POLLIN) != 0 {
                let mut buf = [0u8; 1024];
                libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len());
            }
        }
    }

    pub fn draw_rect(&self, x: i32, y: i32, width: i32, height: i32, color: [f32; 4]) {
        unsafe {
            self.gl.use_program(Some(self.color_program));

            let offset_loc = self.gl.get_uniform_location(self.color_program, "u_offset");
            let size_loc = self.gl.get_uniform_location(self.color_program, "u_size");
            let screen_loc = self
                .gl
                .get_uniform_location(self.color_program, "u_screen_size");
            let color_loc = self.gl.get_uniform_location(self.color_program, "u_color");

            self.gl
                .uniform_2_f32(offset_loc.as_ref(), x as f32, y as f32);
            self.gl
                .uniform_2_f32(size_loc.as_ref(), width as f32, height as f32);
            self.gl
                .uniform_2_f32(screen_loc.as_ref(), self.width as f32, self.height as f32);
            self.gl
                .uniform_4_f32(color_loc.as_ref(), color[0], color[1], color[2], color[3]);

            self.gl.bind_vertex_array(Some(self.quad_vao));
            self.gl.draw_arrays(glow::TRIANGLES, 0, 6);
        }
    }

    pub fn upload_shm_texture(
        &mut self,
        id: u64,
        width: u32,
        height: u32,
        stride: u32,
        data: &[u8],
    ) -> glow::Texture {
        unsafe {
            if let Some(old_tex) = self.shm_textures.remove(&id) {
                self.gl.delete_texture(old_tex);
            }

            let texture = self.gl.create_texture().unwrap();
            self.shm_textures.insert(id, texture);

            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );

            self.gl
                .pixel_store_i32(glow::UNPACK_ROW_LENGTH, stride as i32);

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

            self.gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);

            texture
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn import_dmabuf_texture(
        &mut self,
        id: u64,
        fd: RawFd,
        width: u32,
        height: u32,
        format: u32,
        stride: u32,
        offset: u32,
        modifier: u64,
    ) -> Option<glow::Texture> {
        if let Some((tex, _, _)) = self.dmabuf_textures.get(&id) {
            return Some(*tex);
        }

        log::info!("[gpu] Creating new DMA-BUF texture (single plane): id={} fd={} {}x{} format={:#x} modifier={:#x}",
            id, fd, width, height, format, modifier);

        let mut attribs: Vec<egl::Attrib> = vec![
            egl::WIDTH as egl::Attrib,
            width as egl::Attrib,
            egl::HEIGHT as egl::Attrib,
            height as egl::Attrib,
            EGL_LINUX_DRM_FOURCC_EXT as egl::Attrib,
            format as egl::Attrib,
        ];

        const MOD_INVALID: u64 = 0x00ffffffffffffff;

        attribs.push(EGL_DMA_BUF_PLANE0_FD_EXT as egl::Attrib);
        attribs.push(fd as egl::Attrib);
        attribs.push(EGL_DMA_BUF_PLANE0_OFFSET_EXT as egl::Attrib);
        attribs.push(offset as egl::Attrib);
        attribs.push(EGL_DMA_BUF_PLANE0_PITCH_EXT as egl::Attrib);
        attribs.push(stride as egl::Attrib);
        if modifier != MOD_INVALID {
            attribs.push(EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT as egl::Attrib);
            attribs.push((modifier & 0xFFFFFFFF) as egl::Attrib);
            attribs.push(EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT as egl::Attrib);
            attribs.push((modifier >> 32) as egl::Attrib);
        }

        attribs.push(egl::NONE as egl::Attrib);

        self.import_dmabuf_texture_from_attribs(id, &attribs)
    }

    pub fn import_dmabuf_texture_multiplane(
        &mut self,
        id: u64,
        width: u32,
        height: u32,
        format: u32,
        planes: &[crate::state::DmaBufPlaneInfo],
    ) -> Option<glow::Texture> {
        if planes.is_empty() {
            log::warn!("[gpu] No planes provided for DMA-BUF import");
            return None;
        }

        if let Some((tex, _, _)) = self.dmabuf_textures.get(&id) {
            return Some(*tex);
        }

        let plane0 = &planes[0];
        let fd0 = plane0.fd.as_raw_fd();
        log::info!("[gpu] Creating new DMA-BUF texture: id={} fd={} {}x{} format={:#x} planes={} modifier={:#x}",
            id, fd0, width, height, format, planes.len(), plane0.modifier);

        let mut attribs: Vec<egl::Attrib> = vec![
            egl::WIDTH as egl::Attrib,
            width as egl::Attrib,
            egl::HEIGHT as egl::Attrib,
            height as egl::Attrib,
            EGL_LINUX_DRM_FOURCC_EXT as egl::Attrib,
            format as egl::Attrib,
        ];

        const MOD_INVALID: u64 = 0x00ffffffffffffff;

        attribs.push(EGL_DMA_BUF_PLANE0_FD_EXT as egl::Attrib);
        attribs.push(fd0 as egl::Attrib);
        attribs.push(EGL_DMA_BUF_PLANE0_OFFSET_EXT as egl::Attrib);
        attribs.push(plane0.offset as egl::Attrib);
        attribs.push(EGL_DMA_BUF_PLANE0_PITCH_EXT as egl::Attrib);
        attribs.push(plane0.stride as egl::Attrib);
        if plane0.modifier != MOD_INVALID {
            attribs.push(EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT as egl::Attrib);
            attribs.push((plane0.modifier & 0xFFFFFFFF) as egl::Attrib);
            attribs.push(EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT as egl::Attrib);
            attribs.push((plane0.modifier >> 32) as egl::Attrib);
        }

        if planes.len() > 1 {
            let plane1 = &planes[1];
            let fd1 = plane1.fd.as_raw_fd();
            attribs.push(EGL_DMA_BUF_PLANE1_FD_EXT as egl::Attrib);
            attribs.push(fd1 as egl::Attrib);
            attribs.push(EGL_DMA_BUF_PLANE1_OFFSET_EXT as egl::Attrib);
            attribs.push(plane1.offset as egl::Attrib);
            attribs.push(EGL_DMA_BUF_PLANE1_PITCH_EXT as egl::Attrib);
            attribs.push(plane1.stride as egl::Attrib);
            if plane1.modifier != MOD_INVALID {
                attribs.push(EGL_DMA_BUF_PLANE1_MODIFIER_LO_EXT as egl::Attrib);
                attribs.push((plane1.modifier & 0xFFFFFFFF) as egl::Attrib);
                attribs.push(EGL_DMA_BUF_PLANE1_MODIFIER_HI_EXT as egl::Attrib);
                attribs.push((plane1.modifier >> 32) as egl::Attrib);
            }
        }

        if planes.len() > 2 {
            let plane2 = &planes[2];
            let fd2 = plane2.fd.as_raw_fd();
            attribs.push(EGL_DMA_BUF_PLANE2_FD_EXT as egl::Attrib);
            attribs.push(fd2 as egl::Attrib);
            attribs.push(EGL_DMA_BUF_PLANE2_OFFSET_EXT as egl::Attrib);
            attribs.push(plane2.offset as egl::Attrib);
            attribs.push(EGL_DMA_BUF_PLANE2_PITCH_EXT as egl::Attrib);
            attribs.push(plane2.stride as egl::Attrib);
            if plane2.modifier != MOD_INVALID {
                attribs.push(EGL_DMA_BUF_PLANE2_MODIFIER_LO_EXT as egl::Attrib);
                attribs.push((plane2.modifier & 0xFFFFFFFF) as egl::Attrib);
                attribs.push(EGL_DMA_BUF_PLANE2_MODIFIER_HI_EXT as egl::Attrib);
                attribs.push((plane2.modifier >> 32) as egl::Attrib);
            }
        }

        if planes.len() > 3 {
            let plane3 = &planes[3];
            let fd3 = plane3.fd.as_raw_fd();
            attribs.push(EGL_DMA_BUF_PLANE3_FD_EXT as egl::Attrib);
            attribs.push(fd3 as egl::Attrib);
            attribs.push(EGL_DMA_BUF_PLANE3_OFFSET_EXT as egl::Attrib);
            attribs.push(plane3.offset as egl::Attrib);
            attribs.push(EGL_DMA_BUF_PLANE3_PITCH_EXT as egl::Attrib);
            attribs.push(plane3.stride as egl::Attrib);
            if plane3.modifier != MOD_INVALID {
                attribs.push(EGL_DMA_BUF_PLANE3_MODIFIER_LO_EXT as egl::Attrib);
                attribs.push((plane3.modifier & 0xFFFFFFFF) as egl::Attrib);
                attribs.push(EGL_DMA_BUF_PLANE3_MODIFIER_HI_EXT as egl::Attrib);
                attribs.push((plane3.modifier >> 32) as egl::Attrib);
            }
        }

        attribs.push(egl::NONE as egl::Attrib);

        self.import_dmabuf_texture_from_attribs(id, &attribs)
    }

    fn import_dmabuf_texture_from_attribs(
        &mut self,
        id: u64,
        attribs: &[egl::Attrib],
    ) -> Option<glow::Texture> {
        let image = unsafe {
            let no_context = egl::Context::from_ptr(std::ptr::null_mut());
            let no_buffer = egl::ClientBuffer::from_ptr(std::ptr::null_mut());
            match self.egl.create_image(
                self.display,
                no_context,
                EGL_LINUX_DMA_BUF_EXT,
                no_buffer,
                attribs,
            ) {
                Ok(img) => img,
                Err(e) => {
                    log::warn!("[gpu] Failed to create EGL image from DMA-BUF: {:?}", e);
                    return None;
                }
            }
        };

        type GlEglImageTargetTexture2DOesFn = unsafe extern "C" fn(u32, *const std::ffi::c_void);
        let gl_image_target: Option<GlEglImageTargetTexture2DOesFn> = self
            .egl
            .get_proc_address("glEGLImageTargetTexture2DOES")
            .map(|p| unsafe { std::mem::transmute(p) });

        let gl_image_target = match gl_image_target {
            Some(f) => f,
            None => {
                log::warn!("[gpu] glEGLImageTargetTexture2DOES not available");
                self.egl.destroy_image(self.display, image).ok();
                return None;
            }
        };

        // try GL_TEXTURE_2D first - this is a hacky workaround for firefox
        let (texture, is_external) = unsafe {
            let tex = self.gl.create_texture().ok()?;

            self.gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );

            while self.gl.get_error() != glow::NO_ERROR {}

            gl_image_target(glow::TEXTURE_2D, image.as_ptr());

            let gl_error = self.gl.get_error();
            if gl_error == glow::NO_ERROR {
                log::debug!("[gpu] DMA-BUF imported as GL_TEXTURE_2D (id={})", id);
                (tex, false)
            } else {
                log::debug!(
                    "[gpu] GL_TEXTURE_2D failed ({:#x}), trying GL_TEXTURE_EXTERNAL_OES",
                    gl_error
                );

                self.gl.bind_texture(GL_TEXTURE_EXTERNAL_OES, Some(tex));
                self.gl.tex_parameter_i32(
                    GL_TEXTURE_EXTERNAL_OES,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                self.gl.tex_parameter_i32(
                    GL_TEXTURE_EXTERNAL_OES,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                self.gl.tex_parameter_i32(
                    GL_TEXTURE_EXTERNAL_OES,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                self.gl.tex_parameter_i32(
                    GL_TEXTURE_EXTERNAL_OES,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );

                while self.gl.get_error() != glow::NO_ERROR {}

                gl_image_target(GL_TEXTURE_EXTERNAL_OES, image.as_ptr());

                let gl_error = self.gl.get_error();
                if gl_error != glow::NO_ERROR {
                    log::warn!("[gpu] GL_TEXTURE_EXTERNAL_OES also failed: {:#x}", gl_error);
                    self.gl.delete_texture(tex);
                    self.egl.destroy_image(self.display, image).ok();
                    return None;
                }

                log::debug!(
                    "[gpu] DMA-BUF imported as GL_TEXTURE_EXTERNAL_OES (id={})",
                    id
                );
                (tex, true)
            }
        };

        log::debug!(
            "[gpu] Successfully imported DMA-BUF texture id={} (external={})",
            id,
            is_external
        );
        self.dmabuf_textures
            .insert(id, (texture, image, is_external));
        Some(texture)
    }

    #[allow(dead_code)]
    pub fn get_dmabuf_texture(&self, id: u64) -> Option<glow::Texture> {
        self.dmabuf_textures.get(&id).map(|(tex, _, _)| *tex)
    }

    pub fn is_dmabuf_external(&self, id: u64) -> bool {
        self.dmabuf_textures
            .get(&id)
            .map(|(_, _, is_external)| *is_external)
            .unwrap_or(true)
    }

    pub fn draw_texture(&self, texture: glow::Texture, x: i32, y: i32, width: i32, height: i32) {
        unsafe {
            self.gl.enable(glow::BLEND);
            self.gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);

            self.gl.use_program(Some(self.texture_program));

            let offset_loc = self
                .gl
                .get_uniform_location(self.texture_program, "u_offset");
            let size_loc = self.gl.get_uniform_location(self.texture_program, "u_size");
            let screen_loc = self
                .gl
                .get_uniform_location(self.texture_program, "u_screen_size");
            let tex_loc = self
                .gl
                .get_uniform_location(self.texture_program, "u_texture");

            self.gl
                .uniform_2_f32(offset_loc.as_ref(), x as f32, y as f32);
            self.gl
                .uniform_2_f32(size_loc.as_ref(), width as f32, height as f32);
            self.gl
                .uniform_2_f32(screen_loc.as_ref(), self.width as f32, self.height as f32);
            self.gl.uniform_1_i32(tex_loc.as_ref(), 0);

            self.gl.active_texture(glow::TEXTURE0);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));

            self.gl.bind_vertex_array(Some(self.quad_vao));
            self.gl.draw_arrays(glow::TRIANGLES, 0, 6);

            self.gl.disable(glow::BLEND);
        }
    }

    pub fn draw_dmabuf_texture(
        &self,
        texture: glow::Texture,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        is_external: bool,
    ) {
        if is_external {
            let program = match self.external_program {
                Some(p) => p,
                None => {
                    log::warn!("[gpu] Cannot draw external DMA-BUF texture: external program not available");
                    return;
                }
            };

            unsafe {
                self.gl.enable(glow::BLEND);
                self.gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);

                self.gl.use_program(Some(program));

                let offset_loc = self.gl.get_uniform_location(program, "u_offset");
                let size_loc = self.gl.get_uniform_location(program, "u_size");
                let screen_loc = self.gl.get_uniform_location(program, "u_screen_size");
                let tex_loc = self.gl.get_uniform_location(program, "u_texture");

                self.gl
                    .uniform_2_f32(offset_loc.as_ref(), x as f32, y as f32);
                self.gl
                    .uniform_2_f32(size_loc.as_ref(), width as f32, height as f32);
                self.gl
                    .uniform_2_f32(screen_loc.as_ref(), self.width as f32, self.height as f32);
                self.gl.uniform_1_i32(tex_loc.as_ref(), 0);

                self.gl.active_texture(glow::TEXTURE0);
                self.gl.bind_texture(GL_TEXTURE_EXTERNAL_OES, Some(texture));

                self.gl.bind_vertex_array(Some(self.quad_vao));
                self.gl.draw_arrays(glow::TRIANGLES, 0, 6);

                let gl_error = self.gl.get_error();
                if gl_error != glow::NO_ERROR {
                    log::warn!(
                        "[gpu] GL error after drawing external DMA-BUF texture: {:#x}",
                        gl_error
                    );
                }

                self.gl.disable(glow::BLEND);
            }
        } else {
            unsafe {
                self.gl.enable(glow::BLEND);
                self.gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);

                self.gl.use_program(Some(self.dmabuf_program));

                let offset_loc = self
                    .gl
                    .get_uniform_location(self.dmabuf_program, "u_offset");
                let size_loc = self.gl.get_uniform_location(self.dmabuf_program, "u_size");
                let screen_loc = self
                    .gl
                    .get_uniform_location(self.dmabuf_program, "u_screen_size");
                let tex_loc = self
                    .gl
                    .get_uniform_location(self.dmabuf_program, "u_texture");

                self.gl
                    .uniform_2_f32(offset_loc.as_ref(), x as f32, y as f32);
                self.gl
                    .uniform_2_f32(size_loc.as_ref(), width as f32, height as f32);
                self.gl
                    .uniform_2_f32(screen_loc.as_ref(), self.width as f32, self.height as f32);
                self.gl.uniform_1_i32(tex_loc.as_ref(), 0);

                self.gl.active_texture(glow::TEXTURE0);
                self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));

                self.gl.bind_vertex_array(Some(self.quad_vao));
                self.gl.draw_arrays(glow::TRIANGLES, 0, 6);

                let gl_error = self.gl.get_error();
                if gl_error != glow::NO_ERROR {
                    log::warn!(
                        "[gpu] GL error after drawing DMA-BUF texture: {:#x}",
                        gl_error
                    );
                }

                self.gl.disable(glow::BLEND);
            }
        }
    }

    pub fn draw_cursor(&mut self, x: i32, y: i32) {
        // W = white, B = black outline, . = transparent
        const CURSOR: &[&str] = &[
            "BW",
            "BWWB",
            "BWWWB",
            "BWWWWB",
            "BWWWWWB",
            "BWWWWWWB",
            "BWWWWWWWB",
            "BWWWWWWWWB",
            "BWWWWWWWWWB",
            "BWWWWWWWWWWB",
            "BWWWWWWBBBBB",
            "BWWWBWWB",
            "BWWBBWWWB",
            "BWB.BWWWB",
            "BB..BWWWB",
            "B....BWWWB",
            ".....BWWWB",
            "......BWWB",
            "......BBB",
        ];

        const CURSOR_W: usize = 12;
        const CURSOR_H: usize = 19;

        let cursor_id = u64::MAX - 1;

        if !self.shm_textures.contains_key(&cursor_id) {
            let mut pixels = vec![0u32; CURSOR_W * CURSOR_H];

            for (dy, row) in CURSOR.iter().enumerate() {
                for (dx, ch) in row.chars().enumerate() {
                    let color = match ch {
                        'W' => 0xFFFFFFFF,
                        'B' => 0xFF000000,
                        _ => 0x00000000,
                    };
                    pixels[dy * CURSOR_W + dx] = color;
                }
            }

            let data: &[u8] = unsafe {
                std::slice::from_raw_parts(pixels.as_ptr() as *const u8, pixels.len() * 4)
            };

            self.upload_shm_texture(
                cursor_id,
                CURSOR_W as u32,
                CURSOR_H as u32,
                (CURSOR_W * 4) as u32,
                data,
            );
        }

        if let Some(&texture) = self.shm_textures.get(&cursor_id) {
            self.draw_texture(texture, x, y, CURSOR_W as i32, CURSOR_H as i32);
        }
    }

    pub fn remove_texture(&mut self, id: u64) {
        if let Some(tex) = self.shm_textures.remove(&id) {
            unsafe {
                self.gl.delete_texture(tex);
            }
        }
        if let Some((tex, img, _)) = self.dmabuf_textures.remove(&id) {
            unsafe {
                self.gl.delete_texture(tex);
                self.egl.destroy_image(self.display, img).ok();
            }
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn physical_size(&self) -> (u32, u32) {
        (self.physical_width, self.physical_height)
    }

    pub fn drm_fd(&self) -> BorrowedFd<'_> {
        self.drm_card.as_fd()
    }

    pub fn is_flip_pending(&self) -> bool {
        self.flip_pending
    }

    pub fn handle_drm_event(&mut self) -> bool {
        if !self.flip_pending {
            return false;
        }

        use std::os::fd::AsRawFd;
        let fd = self.drm_card.as_fd().as_raw_fd();

        unsafe {
            let mut fds = [libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            }];

            let ret = libc::poll(fds.as_mut_ptr(), 1, 0);
            if ret > 0 && (fds[0].revents & libc::POLLIN) != 0 {
                let mut buf = [0u8; 1024];
                libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len());

                let gbm_surface = self.gbm_surface as *mut gbm_sys::gbm_surface;
                if let Some(old_fb) = self.current_fb.take() {
                    self.drm_card.destroy_framebuffer(old_fb).ok();
                }
                if !self.current_bo.is_null() {
                    gbm_sys::gbm_surface_release_buffer(gbm_surface, self.current_bo);
                }
                self.current_fb = self.next_fb.take();
                self.current_bo = self.next_bo;
                self.next_bo = std::ptr::null_mut();
                self.flip_pending = false;

                return true;
            }
        }
        false
    }

    pub fn drm_dev(&self) -> u64 {
        use std::os::fd::AsRawFd;

        let fd = self.drm_card.as_fd().as_raw_fd();
        unsafe {
            let mut stat: libc::stat = std::mem::zeroed();
            if libc::fstat(fd, &mut stat) == 0 {
                stat.st_rdev
            } else {
                0
            }
        }
    }

    pub fn render_node_dev(&self) -> u64 {
        let card_dev = self.drm_dev();
        if card_dev == 0 {
            return 0;
        }

        let card_minor = (card_dev & 0xff) as u32;
        let render_minor = 128 + card_minor;
        let render_path = format!("/dev/dri/renderD{}", render_minor);

        if let Ok(meta) = std::fs::metadata(&render_path) {
            use std::os::unix::fs::MetadataExt;
            return meta.rdev();
        }

        for path in &["/dev/dri/renderD128", "/dev/dri/renderD129"] {
            if let Ok(meta) = std::fs::metadata(path) {
                use std::os::unix::fs::MetadataExt;
                return meta.rdev();
            }
        }

        card_dev
    }

    pub fn read_pixels(&self, x: i32, y: i32, width: i32, height: i32) -> Vec<u32> {
        let mut pixels = vec![0u32; (width * height) as usize];
        unsafe {
            self.gl.read_pixels(
                x,
                (self.height as i32) - y - height,
                width,
                height,
                glow::BGRA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(std::slice::from_raw_parts_mut(
                    pixels.as_mut_ptr() as *mut u8,
                    pixels.len() * 4,
                ))),
            );
        }

        let mut flipped = vec![0u32; pixels.len()];
        for row in 0..height as usize {
            let src_row = (height as usize - 1 - row) * width as usize;
            let dst_row = row * width as usize;
            flipped[dst_row..dst_row + width as usize]
                .copy_from_slice(&pixels[src_row..src_row + width as usize]);
        }
        flipped
    }
}

impl Drop for GpuRenderer {
    fn drop(&mut self) {
        for (_, tex) in self.shm_textures.drain() {
            unsafe {
                self.gl.delete_texture(tex);
            }
        }
        for (_, (tex, img, _)) in self.dmabuf_textures.drain() {
            unsafe {
                self.gl.delete_texture(tex);
                self.egl.destroy_image(self.display, img).ok();
            }
        }

        unsafe {
            self.gl.delete_program(self.texture_program);
            if let Some(external) = self.external_program {
                self.gl.delete_program(external);
            }
            self.gl.delete_program(self.dmabuf_program);
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
    unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, std::mem::size_of_val(data)) }
}

pub struct ProfilerStats {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub render_time_us: u64,
    pub input_time_us: u64,
    pub memory_mb: f32,
    pub window_count: usize,
    pub texture_count: usize,
}

const FONT_DATA: &[u8] = include_bytes!("font5x7.raw");
const FONT_CHAR_WIDTH: usize = 5;
const FONT_CHAR_HEIGHT: usize = 7;
const FONT_CHARS_PER_ROW: usize = 16;
const PROFILER_TEXTURE_ID: u64 = u64::MAX - 1;

impl GpuRenderer {
    pub fn draw_profiler(&mut self, stats: &ProfilerStats) {
        let lines = [
            format!("FPS: {:.1}", stats.fps),
            format!("Frame: {:.2}ms", stats.frame_time_ms),
            format!("Render: {}us", stats.render_time_us),
            format!("Input: {}us", stats.input_time_us),
            format!("Mem: {:.1}MB", stats.memory_mb),
            format!("Windows: {}", stats.window_count),
            format!("Textures: {}", stats.texture_count),
        ];

        let scale: usize = 2;
        let char_w = FONT_CHAR_WIDTH * scale;
        let char_h = FONT_CHAR_HEIGHT * scale;
        let line_height = char_h + 2;
        let padding: usize = 8;

        let max_chars = lines.iter().map(|l| l.len()).max().unwrap_or(0);
        let box_width = max_chars * char_w + padding * 2;
        let box_height = lines.len() * line_height + padding * 2;
        let mut pixels = vec![0u8; box_width * box_height * 4];

        for i in 0..(box_width * box_height) {
            pixels[i * 4] = 0;
            pixels[i * 4 + 1] = 0;
            pixels[i * 4 + 2] = 0;
            pixels[i * 4 + 3] = 180;
        }

        for (line_idx, line) in lines.iter().enumerate() {
            let text_y = padding + line_idx * line_height;
            for (char_idx, ch) in line.chars().enumerate() {
                let text_x = padding + char_idx * char_w;
                Self::draw_char_to_buffer(&mut pixels, box_width, text_x, text_y, ch, scale);
            }
        }

        let texture = self.upload_profiler_texture(box_width as u32, box_height as u32, &pixels);

        let box_x = self.width as i32 - box_width as i32 - 10;
        let box_y = 10;

        self.draw_texture(texture, box_x, box_y, box_width as i32, box_height as i32);
    }

    fn draw_char_to_buffer(
        pixels: &mut [u8],
        stride: usize,
        x: usize,
        y: usize,
        ch: char,
        scale: usize,
    ) {
        let idx = if ch.is_ascii() && ch >= ' ' {
            (ch as usize) - 32
        } else {
            0
        };

        let font_x = (idx % FONT_CHARS_PER_ROW) * FONT_CHAR_WIDTH;
        let font_y = (idx / FONT_CHARS_PER_ROW) * FONT_CHAR_HEIGHT;

        for cy in 0..FONT_CHAR_HEIGHT {
            for cx in 0..FONT_CHAR_WIDTH {
                let px = font_x + cx;
                let py = font_y + cy;
                let byte_idx = py * (FONT_CHARS_PER_ROW * FONT_CHAR_WIDTH) + px;

                if byte_idx < FONT_DATA.len() && FONT_DATA[byte_idx] > 127 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let screen_x = x + cx * scale + sx;
                            let screen_y = y + cy * scale + sy;
                            let pixel_idx = (screen_y * stride + screen_x) * 4;
                            if pixel_idx + 3 < pixels.len() {
                                pixels[pixel_idx] = 255;
                                pixels[pixel_idx + 1] = 255;
                                pixels[pixel_idx + 2] = 255;
                                pixels[pixel_idx + 3] = 255;
                            }
                        }
                    }
                }
            }
        }
    }

    fn upload_profiler_texture(&mut self, width: u32, height: u32, data: &[u8]) -> glow::Texture {
        unsafe {
            let texture = if let Some(&tex) = self.shm_textures.get(&PROFILER_TEXTURE_ID) {
                tex
            } else {
                let tex = self.gl.create_texture().unwrap();
                self.shm_textures.insert(PROFILER_TEXTURE_ID, tex);
                tex
            };

            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );

            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(data)),
            );

            texture
        }
    }

    pub fn texture_count(&self) -> usize {
        self.shm_textures.len() + self.dmabuf_textures.len()
    }
}
