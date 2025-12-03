use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::gbm::GbmDevice;
use smithay::backend::allocator::Fourcc;
use smithay::backend::egl::context::{GlAttributes, PixelFormatRequirements};
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::{Bind, Frame, ImportDma, ImportMem, Renderer, Texture};
use smithay::backend::renderer::Color32F;
use smithay::utils::{Point, Rectangle, Size, Transform};

use drm::control::{connector, crtc, framebuffer, Device as ControlDevice};
use drm_fourcc::{DrmFourcc, DrmModifier};

use smithay::reexports::gbm::{BufferObject, BufferObjectFlags};

#[derive(Clone, Debug)]
pub struct DmaBufFormat {
    pub format: u32,
    pub modifier: u64,
}

enum RenderCommand {
    Clear {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: [f32; 4],
    },
    Texture {
        texture_id: u64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        is_dmabuf: bool,
    },
}

pub struct GpuRenderer {
    renderer: GlesRenderer,
    #[allow(dead_code)]
    egl_display: EGLDisplay,
    drm_device: std::fs::File,
    drm_fd: i32,
    #[allow(dead_code)]
    gbm: GbmDevice<std::fs::File>,
    width: u32,
    height: u32,
    physical_width: u32,
    physical_height: u32,
    mode: drm::control::Mode,
    connector: connector::Handle,
    crtc: crtc::Handle,
    render_buffers: [RenderBuffer; 2],
    current_buffer: usize,
    mode_set: bool,
    flip_pending: bool,
    pending_fb: Option<framebuffer::Handle>,
    current_fb: Option<framebuffer::Handle>,
    shm_textures: HashMap<u64, GlesTexture>,
    dmabuf_textures: HashMap<u64, GlesTexture>,
    render_commands: Vec<RenderCommand>,
    pub supported_formats: Vec<DmaBufFormat>,
}

struct RenderBuffer {
    #[allow(dead_code)]
    bo: BufferObject<()>,
    dmabuf: Dmabuf,
    fb: Option<framebuffer::Handle>,
}

struct DrmCard(std::fs::File);

impl AsFd for DrmCard {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for DrmCard {}
impl ControlDevice for DrmCard {}

impl GpuRenderer {
    pub fn new(drm_device: std::fs::File) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_with_config(drm_device, None, true)
    }

    pub fn new_with_config(
        drm_device: std::fs::File,
        preferred_mode: Option<(u16, u16, Option<u32>)>,
        _vsync: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let gbm = GbmDevice::new(drm_device.try_clone()?)?;
        let card = DrmCard(drm_device.try_clone()?);
        let resources = card.resource_handles()?;
        let connectors: Vec<_> = resources
            .connectors()
            .iter()
            .filter_map(|&c| card.get_connector(c, true).ok())
            .collect();

        let connector_info = connectors
            .iter()
            .find(|c| c.state() == connector::State::Connected)
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
        log::info!(
            "[gpu] Selected mode: {}x{}@{}Hz",
            width,
            height,
            mode.vrefresh()
        );

        let crtc_handle = resources.crtcs().first().copied().ok_or("No CRTC available")?;

        let gbm_for_egl = GbmDevice::new(drm_device.try_clone()?)?;

        let egl_display = unsafe { EGLDisplay::new(gbm_for_egl) }
            .map_err(|e| format!("EGL display failed: {:?}", e))?;

        let gl_attrs = GlAttributes {
            version: (3, 0),
            profile: None,
            debug: false,
            vsync: false,
        };
        let egl_context =
            EGLContext::new_with_config(&egl_display, gl_attrs, PixelFormatRequirements::_10_bit())
                .or_else(|_| EGLContext::new_with_config(&egl_display, gl_attrs, PixelFormatRequirements::_8_bit()))
                .map_err(|e| format!("EGL context failed: {:?}", e))?;

        let renderer = unsafe { GlesRenderer::new(egl_context) }
            .map_err(|e| format!("GLES renderer failed: {:?}", e))?;

        log::info!("[gpu] Smithay GLES renderer initialized");

        let supported_formats = Self::query_dmabuf_formats(&egl_display);
        log::info!(
            "[gpu] Supported DMA-BUF formats: {}",
            supported_formats.len()
        );

        let render_buffers = [
            Self::create_render_buffer(&gbm, &card, width, height)?,
            Self::create_render_buffer(&gbm, &card, width, height)?,
        ];

        let drm_fd = drm_device.as_raw_fd();

        Ok(Self {
            renderer,
            egl_display,
            drm_device,
            drm_fd,
            gbm,
            width,
            height,
            physical_width,
            physical_height,
            mode,
            connector: connector_handle,
            crtc: crtc_handle,
            render_buffers,
            current_buffer: 0,
            mode_set: false,
            flip_pending: false,
            pending_fb: None,
            current_fb: None,
            shm_textures: HashMap::new(),
            dmabuf_textures: HashMap::new(),
            render_commands: Vec::with_capacity(64),
            supported_formats,
        })
    }

    fn create_render_buffer(
        gbm: &GbmDevice<std::fs::File>,
        card: &DrmCard,
        width: u32,
        height: u32,
    ) -> Result<RenderBuffer, Box<dyn std::error::Error>> {
        use smithay::reexports::gbm::Format as GbmFormat;

        let bo = gbm
            .create_buffer_object::<()>(
                width,
                height,
                GbmFormat::Xrgb8888,
                BufferObjectFlags::SCANOUT | BufferObjectFlags::RENDERING,
            )
            .map_err(|e| format!("Failed to create GBM buffer: {:?}", e))?;

        let fd = bo.fd().map_err(|e| format!("Failed to get BO fd: {:?}", e))?;
        let stride = bo.stride();
        let modifier: DrmModifier = bo.modifier().into();

        let mut builder = Dmabuf::builder(
            (width as i32, height as i32),
            DrmFourcc::Xrgb8888,
            modifier.into(),
            smithay::backend::allocator::dmabuf::DmabufFlags::empty(),
        );

        let plane_fd = unsafe { OwnedFd::from_raw_fd(libc::dup(fd.as_raw_fd())) };
        if !builder.add_plane(plane_fd, 0, 0, stride) {
            return Err("Failed to add plane to Dmabuf".into());
        }

        let dmabuf = builder.build().ok_or("Failed to build Dmabuf")?;

        let handle = unsafe { bo.handle().u32_ };
        let fb = card
            .add_framebuffer(
                &DrmBuffer {
                    handle,
                    width,
                    height,
                    stride,
                },
                24,
                32,
            )
            .map_err(|e| format!("Failed to create framebuffer: {:?}", e))?;

        Ok(RenderBuffer {
            bo,
            dmabuf,
            fb: Some(fb),
        })
    }

    fn query_dmabuf_formats(egl_display: &EGLDisplay) -> Vec<DmaBufFormat> {
        let mut formats = Vec::new();

        let egl_formats = egl_display.dmabuf_render_formats();
        for format in egl_formats.iter() {
            formats.push(DmaBufFormat {
                format: format.code as u32,
                modifier: format.modifier.into(),
            });
        }

        if formats.is_empty() {
            formats = vec![
                DmaBufFormat {
                    format: DrmFourcc::Argb8888 as u32,
                    modifier: DrmModifier::Invalid.into(),
                },
                DmaBufFormat {
                    format: DrmFourcc::Xrgb8888 as u32,
                    modifier: DrmModifier::Invalid.into(),
                },
                DmaBufFormat {
                    format: DrmFourcc::Argb8888 as u32,
                    modifier: DrmModifier::Linear.into(),
                },
                DmaBufFormat {
                    format: DrmFourcc::Xrgb8888 as u32,
                    modifier: DrmModifier::Linear.into(),
                },
            ];
        }

        formats
    }

    pub fn begin_frame(&mut self) {
        if self.flip_pending {
            self.wait_for_flip();
            self.flip_pending = false;
            self.current_fb = self.pending_fb.take();
        }

        self.render_commands.clear();
    }

    pub fn end_frame(&mut self) {
        let fb = match self.render_buffers[self.current_buffer].fb {
            Some(fb) => fb,
            None => {
                log::error!("[gpu] No framebuffer for current buffer");
                return;
            }
        };

        let dmabuf = &mut self.render_buffers[self.current_buffer].dmabuf;
        let output_size = Size::from((self.width as i32, self.height as i32));

        if let Ok(mut target) = self.renderer.bind(dmabuf) {
            if let Ok(mut frame) = self.renderer.render(&mut target, output_size, Transform::Normal) {
                for cmd in &self.render_commands {
                    match cmd {
                        RenderCommand::Clear { x, y, width, height, color } => {
                            let rect = Rectangle::new(
                                Point::from((*x, *y)),
                                Size::from((*width, *height)),
                            );
                            let _ = frame.clear(Color32F::from(*color), &[rect]);
                        }
                        RenderCommand::Texture { texture_id, x, y, width, height, is_dmabuf } => {
                            let texture = if *is_dmabuf {
                                self.dmabuf_textures.get(texture_id)
                            } else {
                                self.shm_textures.get(texture_id)
                            };
                            
                            if let Some(texture) = texture {
                                let tex_size = texture.size();
                                let src = Rectangle::new(
                                    Point::from((0.0, 0.0)),
                                    Size::from((tex_size.w as f64, tex_size.h as f64)),
                                );
                                let dst = Rectangle::new(
                                    Point::from((*x, *y)),
                                    Size::from((*width, *height)),
                                );
                                let damage = [dst];
                                let opaque_regions: [Rectangle<i32, smithay::utils::Physical>; 0] = [];
                                
                                let _ = frame.render_texture_from_to(
                                    texture,
                                    src,
                                    dst,
                                    &damage,
                                    &opaque_regions,
                                    Transform::Normal,
                                    1.0,
                                    None,
                                    &[],
                                );
                            }
                        }
                    }
                }
                
                let _ = frame.finish();
            }
        }

        let card = match self.drm_device.try_clone().map(DrmCard) {
            Ok(c) => c,
            Err(e) => {
                log::error!("[gpu] Failed to clone DRM device: {:?}", e);
                return;
            }
        };

        if !self.mode_set {
            if let Err(e) = card.set_crtc(
                self.crtc,
                Some(fb),
                (0, 0),
                &[self.connector],
                Some(self.mode),
            ) {
                log::error!("[gpu] set_crtc failed: {}", e);
                return;
            }
            self.mode_set = true;
            self.current_fb = Some(fb);
        } else {
            use drm::control::PageFlipFlags;

            match card.page_flip(self.crtc, fb, PageFlipFlags::EVENT, None) {
                Ok(()) => {
                    self.pending_fb = Some(fb);
                    self.flip_pending = true;
                }
                Err(e) => {
                    log::warn!("[gpu] page_flip failed: {}, falling back to set_crtc", e);
                    if let Err(e) = card.set_crtc(
                        self.crtc,
                        Some(fb),
                        (0, 0),
                        &[self.connector],
                        Some(self.mode),
                    ) {
                        log::error!("[gpu] set_crtc fallback failed: {}", e);
                        return;
                    }
                    self.current_fb = Some(fb);
                }
            }
        }

        self.current_buffer = 1 - self.current_buffer;
    }

    fn wait_for_flip(&self) {
        let mut fds = [libc::pollfd {
            fd: self.drm_fd,
            events: libc::POLLIN,
            revents: 0,
        }];

        let timeout_ms = 16;

        unsafe {
            let ret = libc::poll(fds.as_mut_ptr(), 1, timeout_ms);
            if ret > 0 && (fds[0].revents & libc::POLLIN) != 0 {
                let mut buf = [0u8; 1024];
                libc::read(self.drm_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len());
            }
        }
    }

    pub fn draw_rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: [f32; 4]) {
        self.render_commands.push(RenderCommand::Clear {
            x,
            y,
            width,
            height,
            color,
        });
    }

    pub fn upload_shm_texture(
        &mut self,
        id: u64,
        width: u32,
        height: u32,
        _stride: u32,
        data: &[u8],
    ) -> GlesTexture {
        self.shm_textures.remove(&id);

        let format = Fourcc::Argb8888;
        let size = Size::from((width as i32, height as i32));

        match self.renderer.import_memory(data, format, size, false) {
            Ok(texture) => {
                self.shm_textures.insert(id, texture.clone());
                texture
            }
            Err(e) => {
                log::error!("[gpu] Failed to upload SHM texture: {:?}", e);
                panic!("Failed to create texture");
            }
        }
    }

    pub fn import_dmabuf_texture(
        &mut self,
        id: u64,
        fd: i32,
        width: u32,
        height: u32,
        format: u32,
        stride: u32,
        offset: u32,
        modifier: u64,
    ) -> Option<GlesTexture> {
        if let Some(tex) = self.dmabuf_textures.get(&id) {
            return Some(tex.clone());
        }

        let fourcc = DrmFourcc::try_from(format).ok()?;
        let drm_mod = DrmModifier::from(modifier);

        let mut builder = Dmabuf::builder(
            (width as i32, height as i32),
            fourcc,
            drm_mod.into(),
            smithay::backend::allocator::dmabuf::DmabufFlags::empty(),
        );

        let plane_fd = unsafe { OwnedFd::from_raw_fd(libc::dup(fd)) };
        if !builder.add_plane(plane_fd, 0, offset, stride) {
            log::warn!("[gpu] Failed to add plane to DMA-BUF");
            return None;
        }

        let dmabuf = builder.build()?;

        match self.renderer.import_dmabuf(&dmabuf, None) {
            Ok(texture) => {
                self.dmabuf_textures.insert(id, texture.clone());
                Some(texture)
            }
            Err(e) => {
                log::warn!("[gpu] Failed to import DMA-BUF: {:?}", e);
                None
            }
        }
    }

    pub fn import_dmabuf_texture_multiplane(
        &mut self,
        id: u64,
        width: u32,
        height: u32,
        format: u32,
        planes: &[crate::state::DmaBufPlaneInfo],
    ) -> Option<GlesTexture> {
        if planes.is_empty() {
            return None;
        }

        if let Some(tex) = self.dmabuf_textures.get(&id) {
            return Some(tex.clone());
        }

        let fourcc = DrmFourcc::try_from(format).ok()?;

        let drm_mod = DrmModifier::from(planes[0].modifier);

        let mut builder = Dmabuf::builder(
            (width as i32, height as i32),
            fourcc,
            drm_mod.into(),
            smithay::backend::allocator::dmabuf::DmabufFlags::empty(),
        );

        for (i, plane) in planes.iter().enumerate() {
            let fd = unsafe { OwnedFd::from_raw_fd(libc::dup(plane.fd.as_raw_fd())) };
            if !builder.add_plane(fd, i as u32, plane.offset, plane.stride) {
                log::warn!("[gpu] Failed to add plane {} to multi-plane DMA-BUF", i);
                return None;
            }
        }

        let dmabuf = builder.build()?;

        match self.renderer.import_dmabuf(&dmabuf, None) {
            Ok(texture) => {
                self.dmabuf_textures.insert(id, texture.clone());
                Some(texture)
            }
            Err(e) => {
                log::warn!("[gpu] Failed to import multi-plane DMA-BUF: {:?}", e);
                None
            }
        }
    }

    pub fn is_format_supported(&self, format: u32, modifier: u64) -> bool {
        const MOD_INVALID: u64 = 0x00ffffffffffffff;
        self.supported_formats.iter().any(|f| {
            f.format == format
                && (f.modifier == modifier
                    || modifier == MOD_INVALID
                    || f.modifier == MOD_INVALID)
        })
    }

    pub fn is_dmabuf_external(&self, _id: u64) -> bool {
        false
    }

    pub fn draw_texture(
        &mut self,
        texture: GlesTexture,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) {
        let texture_id = self.shm_textures.iter()
            .find(|(_, tex)| std::ptr::eq(*tex as *const _, &texture as *const _))
            .map(|(id, _)| *id);
        
        if let Some(id) = texture_id {
            self.render_commands.push(RenderCommand::Texture {
                texture_id: id,
                x,
                y,
                width,
                height,
                is_dmabuf: false,
            });
        } else {
            // TODO this is technically not best practice
            // and probably inefficient, ill address it
            // later probably if i remember
            let temp_id = u64::MAX - 100 - self.render_commands.len() as u64;
            self.shm_textures.insert(temp_id, texture);
            self.render_commands.push(RenderCommand::Texture {
                texture_id: temp_id,
                x,
                y,
                width,
                height,
                is_dmabuf: false,
            });
        }
    }

    pub fn draw_dmabuf_texture(
        &mut self,
        texture: GlesTexture,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        _is_external: bool,
    ) {
        let texture_id = self.dmabuf_textures.iter()
            .find(|(_, tex)| std::ptr::eq(*tex as *const _, &texture as *const _))
            .map(|(id, _)| *id);
        
        if let Some(id) = texture_id {
            self.render_commands.push(RenderCommand::Texture {
                texture_id: id,
                x,
                y,
                width,
                height,
                is_dmabuf: true,
            });
        } else {
            let temp_id = u64::MAX - 200 - self.render_commands.len() as u64;
            self.dmabuf_textures.insert(temp_id, texture);
            self.render_commands.push(RenderCommand::Texture {
                texture_id: temp_id,
                x,
                y,
                width,
                height,
                is_dmabuf: true,
            });
        }
    }

    pub fn draw_cursor(&mut self, x: i32, y: i32) {
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
            let mut pixels = vec![0u8; CURSOR_W * CURSOR_H * 4];

            for (dy, row) in CURSOR.iter().enumerate() {
                for (dx, ch) in row.chars().enumerate() {
                    let (r, g, b, a) = match ch {
                        'W' => (255, 255, 255, 255),
                        'B' => (0, 0, 0, 255),
                        _ => (0, 0, 0, 0),
                    };
                    let idx = (dy * CURSOR_W + dx) * 4;
                    pixels[idx] = b;
                    pixels[idx + 1] = g;
                    pixels[idx + 2] = r;
                    pixels[idx + 3] = a;
                }
            }

            self.upload_shm_texture(
                cursor_id,
                CURSOR_W as u32,
                CURSOR_H as u32,
                (CURSOR_W * 4) as u32,
                &pixels,
            );
        }

        self.render_commands.push(RenderCommand::Texture {
            texture_id: cursor_id,
            x,
            y,
            width: CURSOR_W as i32,
            height: CURSOR_H as i32,
            is_dmabuf: false,
        });
    }

    pub fn remove_texture(&mut self, id: u64) {
        self.shm_textures.remove(&id);
        self.dmabuf_textures.remove(&id);
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn physical_size(&self) -> (u32, u32) {
        (self.physical_width, self.physical_height)
    }

    pub fn drm_fd(&self) -> BorrowedFd<'_> {
        self.drm_device.as_fd()
    }

    pub fn is_flip_pending(&self) -> bool {
        self.flip_pending
    }

    pub fn handle_drm_event(&mut self) -> bool {
        if !self.flip_pending {
            return false;
        }

        let mut fds = [libc::pollfd {
            fd: self.drm_fd,
            events: libc::POLLIN,
            revents: 0,
        }];

        unsafe {
            let ret = libc::poll(fds.as_mut_ptr(), 1, 0);
            if ret > 0 && (fds[0].revents & libc::POLLIN) != 0 {
                let mut buf = [0u8; 1024];
                libc::read(self.drm_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len());

                self.current_fb = self.pending_fb.take();
                self.flip_pending = false;

                return true;
            }
        }
        false
    }

    pub fn drm_dev(&self) -> u64 {
        unsafe {
            let mut stat: libc::stat = std::mem::zeroed();
            if libc::fstat(self.drm_fd, &mut stat) == 0 {
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

    pub fn read_pixels(&self, _x: i32, _y: i32, width: i32, height: i32) -> Vec<u32> {
        // TODO: implement using ExportMem (low priority)
        vec![0u32; (width * height) as usize]
    }

    pub fn texture_count(&self) -> usize {
        self.shm_textures.len() + self.dmabuf_textures.len()
    }

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

        let profiler_id = u64::MAX - 2;
        self.upload_shm_texture(
            profiler_id,
            box_width as u32,
            box_height as u32,
            (box_width * 4) as u32,
            &pixels,
        );

        let box_x = self.width as i32 - box_width as i32 - 10;
        let box_y = 10;

        self.render_commands.push(RenderCommand::Texture {
            texture_id: profiler_id,
            x: box_x,
            y: box_y,
            width: box_width as i32,
            height: box_height as i32,
            is_dmabuf: false,
        });
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
}

struct DrmBuffer {
    handle: u32,
    width: u32,
    height: u32,
    stride: u32,
}

impl drm::buffer::Buffer for DrmBuffer {
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

impl Drop for GpuRenderer {
    fn drop(&mut self) {
        self.shm_textures.clear();
        self.dmabuf_textures.clear();

        if let Ok(card) = self.drm_device.try_clone().map(DrmCard) {
            for buffer in &self.render_buffers {
                if let Some(fb) = buffer.fb {
                    card.destroy_framebuffer(fb).ok();
                }
            }
        }
    }
}

const FONT_DATA: &[u8] = include_bytes!("font5x7.raw");
const FONT_CHAR_WIDTH: usize = 5;
const FONT_CHAR_HEIGHT: usize = 7;
const FONT_CHARS_PER_ROW: usize = 16;

pub struct ProfilerStats {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub render_time_us: u64,
    pub input_time_us: u64,
    pub memory_mb: f32,
    pub window_count: usize,
    pub texture_count: usize,
}
