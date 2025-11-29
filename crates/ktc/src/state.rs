use std::collections::HashMap;
use std::os::fd::{OwnedFd, AsFd, AsRawFd};
use std::ptr::NonNull;
use wayland_server::protocol::{wl_surface::WlSurface, wl_buffer::WlBuffer, wl_shm_pool::WlShmPool, wl_callback::WlCallback, wl_keyboard::WlKeyboard, wl_pointer::WlPointer, wl_output::WlOutput};
use wayland_server::Resource;
use wayland_protocols::xdg::shell::server::{xdg_surface::XdgSurface, xdg_toplevel::{XdgToplevel, State as ToplevelState}};
use wayland_protocols_wlr::layer_shell::v1::server::zwlr_layer_surface_v1::{Anchor, ZwlrLayerSurfaceV1, KeyboardInteractivity};
use wayland_server::backend::ObjectId;
use crate::protocols::screencopy::PendingScreencopy;
use crate::config::Config;

pub type WindowId = u64;
pub type OutputId = u64;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rectangle {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rectangle {
    pub fn union(&self, other: &Rectangle) -> Rectangle {
        if self.width == 0 || self.height == 0 {
            return *other;
        }
        if other.width == 0 || other.height == 0 {
            return *self;
        }
        let x1 = self.x.min(other.x);
        let y1 = self.y.min(other.y);
        let x2 = (self.x + self.width).max(other.x + other.width);
        let y2 = (self.y + self.height).max(other.y + other.height);
        Rectangle { x: x1, y: y1, width: x2 - x1, height: y2 - y1 }
    }

    #[allow(dead_code)]
    pub fn intersects(&self, other: &Rectangle) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }
}

#[derive(Clone, Default)]
pub struct DamageTracker {
    regions: Vec<Rectangle>,
    full_damage: bool,
    frame_count: u64,
    last_damage_frame: u64,
    cursor_only: bool,
}

impl DamageTracker {
    pub fn new() -> Self {
        Self {
            regions: Vec::with_capacity(16),
            full_damage: true,
            frame_count: 0,
            last_damage_frame: 0,
            cursor_only: false,
        }
    }

    pub fn add_damage(&mut self, rect: Rectangle) {
        if rect.is_empty() {
            return;
        }
        self.cursor_only = false;
        self.last_damage_frame = self.frame_count;
        if self.regions.len() >= 16 {
            self.full_damage = true;
            self.regions.clear();
        } else {
            self.regions.push(rect);
        }
    }
    
    pub fn add_cursor_damage(&mut self) {
        if !self.full_damage && self.regions.is_empty() {
            self.cursor_only = true;
        }
        self.last_damage_frame = self.frame_count;
    }

    pub fn mark_full_damage(&mut self) {
        self.full_damage = true;
        self.cursor_only = false;
        self.regions.clear();
        self.last_damage_frame = self.frame_count;
    }

    pub fn has_damage(&self) -> bool {
        self.full_damage || !self.regions.is_empty() || self.cursor_only
    }
    
    pub fn is_cursor_only(&self) -> bool {
        self.cursor_only && !self.full_damage && self.regions.is_empty()
    }

    #[allow(dead_code)]
    pub fn is_full_damage(&self) -> bool {
        self.full_damage
    }

    #[allow(dead_code)]
    pub fn damage_regions(&self) -> &[Rectangle] {
        &self.regions
    }

    pub fn clear(&mut self) {
        self.regions.clear();
        self.full_damage = false;
        self.cursor_only = false;
        self.frame_count += 1;
    }

    pub fn merged_damage(&self, screen_width: i32, screen_height: i32) -> Rectangle {
        if self.full_damage {
            return Rectangle { x: 0, y: 0, width: screen_width, height: screen_height };
        }
        let mut result = Rectangle::default();
        for r in &self.regions {
            result = result.union(r);
        }
        result
    }
}

#[derive(Clone, Debug)]
pub struct Output {
    pub id: OutputId,
    pub name: String,
    pub make: String,
    pub model: String,
    pub x: i32,
    pub y: i32,
    pub physical_width: i32,
    pub physical_height: i32,
    pub width: i32,
    pub height: i32,
    pub refresh: i32,
    pub scale: i32,
    pub transform: OutputTransform,
    pub wl_outputs: Vec<WlOutput>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub enum OutputTransform {
    #[default]
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    FlippedRotate90,
    FlippedRotate180,
    FlippedRotate270,
}

impl Output {
    pub fn new(id: OutputId, name: String, width: i32, height: i32) -> Self {
        Self {
            id,
            name,
            make: "Unknown".to_string(),
            model: "Unknown".to_string(),
            x: 0,
            y: 0,
            physical_width: 0,
            physical_height: 0,
            width,
            height,
            refresh: 60000,
            scale: 1,
            transform: OutputTransform::Normal,
            wl_outputs: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn usable_area(&self) -> Rectangle {
        Rectangle {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }

    #[allow(dead_code)]
    pub fn scaled_size(&self) -> (i32, i32) {
        (self.width / self.scale, self.height / self.scale)
    }
}

pub struct Canvas {
    pub pixels: Vec<u32>,
    pub cursor_save: Vec<u32>,
    pub cursor_save_x: i32,
    pub cursor_save_y: i32,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
}

impl Canvas {
    const CURSOR_W: usize = 16;
    const CURSOR_H: usize = 20;
    
    pub fn new(width: usize, height: usize, bg_color: u32) -> Self {
        let stride = width;
        let pixels = vec![bg_color; width * height];
        Self {
            pixels,
            cursor_save: vec![0; Self::CURSOR_W * Self::CURSOR_H],
            cursor_save_x: -100,
            cursor_save_y: -100,
            width,
            height,
            stride,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize, bg_color: u32) {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.stride = width;
            self.pixels = vec![bg_color; width * height];
            self.cursor_save_x = -100;
            self.cursor_save_y = -100;
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }
    
    pub fn clear_with_pattern(&mut self, bg_dark: u32, bg_light: u32) {
        let tile_size = 32;
        
        let width = self.width;
        let stride = self.stride;
        let pixels = &mut self.pixels;
        
        for y in 0..self.height {
            let ty = y / tile_size;
            let row_start = y * stride;
            let base_color = if ty % 2 == 0 { bg_dark } else { bg_light };
            let alt_color = if ty % 2 == 0 { bg_light } else { bg_dark };
            
            let mut x = 0;
            while x < width {
                let tx = x / tile_size;
                let color = if tx % 2 == 0 { base_color } else { alt_color };
                let tile_end = ((tx + 1) * tile_size).min(width);
                let fill_len = tile_end - x;
                
                let start = row_start + x;
                let end = start + fill_len;
                pixels[start..end].fill(color);
                
                x = tile_end;
            }
        }
    }

    #[allow(dead_code)]
    pub fn draw_border(&mut self, x: i32, y: i32, width: i32, height: i32, color: u32, thickness: i32) {
        let x = x.max(0) as usize;
        let y = y.max(0) as usize;
        let width = width as usize;
        let height = height as usize;
        let thickness = thickness as usize;
        
        for dy in 0..thickness.min(height) {
            for dx in 0..width {
                let px = x + dx;
                let py = y + dy;
                if px < self.width && py < self.height {
                    self.pixels[py * self.stride + px] = color;
                }
            }
        }
        
        for dy in 0..thickness.min(height) {
            for dx in 0..width {
                let px = x + dx;
                let py = y + height.saturating_sub(1 + dy);
                if px < self.width && py < self.height && py >= y {
                    self.pixels[py * self.stride + px] = color;
                }
            }
        }
        
        for dy in 0..height {
            for dx in 0..thickness.min(width) {
                let px = x + dx;
                let py = y + dy;
                if px < self.width && py < self.height {
                    self.pixels[py * self.stride + px] = color;
                }
            }
        }
        
        for dy in 0..height {
            for dx in 0..thickness.min(width) {
                let px = x + width.saturating_sub(1 + dx);
                let py = y + dy;
                if px < self.width && py < self.height && px >= x {
                    self.pixels[py * self.stride + px] = color;
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn blit(&mut self, src: &[u32], src_width: usize, src_height: usize, dst_x: i32, dst_y: i32) {
        let dst_x = dst_x.max(0) as usize;
        let dst_y = dst_y.max(0) as usize;

        for y in 0..src_height {
            let dst_row = dst_y + y;
            if dst_row >= self.height {
                break;
            }

            for x in 0..src_width {
                let dst_col = dst_x + x;
                if dst_col >= self.width {
                    break;
                }

                let src_idx = y * src_width + x;
                let dst_idx = dst_row * self.stride + dst_col;

                if src_idx < src.len() && dst_idx < self.pixels.len() {
                    self.pixels[dst_idx] = src[src_idx];
                }
            }
        }
    }

    pub fn blit_fast(&mut self, src: &[u32], src_width: usize, src_height: usize, src_stride: usize, dst_x: i32, dst_y: i32) {
        let dst_x = dst_x.max(0) as usize;
        let dst_y = dst_y.max(0) as usize;

        for y in 0..src_height.min(self.height.saturating_sub(dst_y)) {
            let dst_row = dst_y + y;
            let src_offset = y * src_stride;
            let dst_offset = dst_row * self.stride + dst_x;
            let copy_width = src_width.min(self.width.saturating_sub(dst_x));

            if src_offset + copy_width <= src.len() && dst_offset + copy_width <= self.pixels.len() {
                self.pixels[dst_offset..dst_offset + copy_width]
                    .copy_from_slice(&src[src_offset..src_offset + copy_width]);
            }
        }
    }

    #[allow(dead_code)]
    pub fn blit_direct(&mut self, src: &[u32], src_width: usize, src_height: usize, src_stride: usize, dst_x: i32, dst_y: i32) {
        if dst_x >= self.width as i32 || dst_y >= self.height as i32 {
            return;
        }
        
        let dst_x_usize = dst_x.max(0) as usize;
        let dst_y_usize = dst_y.max(0) as usize;
        let src_skip_x = if dst_x < 0 { (-dst_x) as usize } else { 0 };
        let src_skip_y = if dst_y < 0 { (-dst_y) as usize } else { 0 };
        
        let actual_src_width = src_width.saturating_sub(src_skip_x);
        let actual_src_height = src_height.saturating_sub(src_skip_y);
        let copy_width = actual_src_width.min(self.width.saturating_sub(dst_x_usize));
        let copy_height = actual_src_height.min(self.height.saturating_sub(dst_y_usize));
        
        if copy_width == 0 || copy_height == 0 {
            return;
        }
        
        let dst_ptr = self.pixels.as_mut_ptr();
        let src_ptr = src.as_ptr();
        
        unsafe {
            for y in 0..copy_height {
                let src_row = src_skip_y + y;
                let dst_row = dst_y_usize + y;
                let src_offset = src_row * src_stride + src_skip_x;
                let dst_offset = dst_row * self.stride + dst_x_usize;
                
                if src_offset + copy_width <= src.len() && dst_offset + copy_width <= self.pixels.len() {
                    std::ptr::copy_nonoverlapping(
                        src_ptr.add(src_offset),
                        dst_ptr.add(dst_offset),
                        copy_width
                    );
                }
            }
        }
    }

    pub fn as_slice(&self) -> &[u32] {
        &self.pixels
    }

    #[allow(dead_code)]
    pub fn as_mut_slice(&mut self) -> &mut [u32] {
        &mut self.pixels
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_decorations(&mut self, x: i32, y: i32, width: i32, height: i32, title_height: i32, is_focused: bool, title_focused: u32, title_unfocused: u32, border_focused: u32, border_unfocused: u32) {
        let title_bg = if is_focused { title_focused } else { title_unfocused };
        let border_color = if is_focused { border_focused } else { border_unfocused };
        
        let x = x.max(0) as usize;
        let y = y.max(0) as usize;
        let width = width as usize;
        let title_height = title_height as usize;
        let total_height = height as usize + title_height;
        
        for dy in 0..title_height {
            for dx in 0..width {
                let px = x + dx;
                let py = y + dy;
                if px < self.width && py < self.height {
                    self.pixels[py * self.stride + px] = title_bg;
                }
            }
        }
        
        for dx in 0..width {
            let px = x + dx;
            if px < self.width && y < self.height {
                self.pixels[y * self.stride + px] = border_color;
            }
        }
        let bottom_y = y + total_height.saturating_sub(1);
        if bottom_y < self.height {
            for dx in 0..width {
                let px = x + dx;
                if px < self.width {
                    self.pixels[bottom_y * self.stride + px] = border_color;
                }
            }
        }
        for dy in 0..total_height {
            let py = y + dy;
            if x < self.width && py < self.height {
                self.pixels[py * self.stride + x] = border_color;
            }
        }
        let right_x = x + width.saturating_sub(1);
        if right_x < self.width {
            for dy in 0..total_height {
                let py = y + dy;
                if py < self.height {
                    self.pixels[py * self.stride + right_x] = border_color;
                }
            }
        }
        
        let title_bottom = y + title_height;
        if title_bottom < self.height {
            for dx in 0..width {
                let px = x + dx;
                if px < self.width {
                    self.pixels[title_bottom * self.stride + px] = border_color;
                }
            }
        }
    }
    
    pub fn draw_cursor(&mut self, x: i32, y: i32) {
        self.save_under_cursor(x, y);
        
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
        
        for (dy, row) in CURSOR.iter().enumerate() {
            for (dx, ch) in row.chars().enumerate() {
                let px = x as usize + dx;
                let py = y as usize + dy;
                if px < self.width && py < self.height {
                    let color = match ch {
                        'W' => 0xFFFFFFFF,
                        'B' => 0xFF000000,
                        _ => continue,
                    };
                    self.pixels[py * self.stride + px] = color;
                }
            }
        }
    }
    
    fn save_under_cursor(&mut self, x: i32, y: i32) {
        self.cursor_save_x = x;
        self.cursor_save_y = y;
        let x = x.max(0) as usize;
        let y = y.max(0) as usize;
        
        for dy in 0..Self::CURSOR_H {
            let py = y + dy;
            if py >= self.height {
                break;
            }
            for dx in 0..Self::CURSOR_W {
                let px = x + dx;
                if px >= self.width {
                    break;
                }
                self.cursor_save[dy * Self::CURSOR_W + dx] = self.pixels[py * self.stride + px];
            }
        }
    }
    
    pub fn restore_cursor(&mut self) {
        if self.cursor_save_x < 0 && self.cursor_save_y < 0 {
            return;
        }
        let x = self.cursor_save_x.max(0) as usize;
        let y = self.cursor_save_y.max(0) as usize;
        
        for dy in 0..Self::CURSOR_H {
            let py = y + dy;
            if py >= self.height {
                break;
            }
            for dx in 0..Self::CURSOR_W {
                let px = x + dx;
                if px >= self.width {
                    break;
                }
                self.pixels[py * self.stride + px] = self.cursor_save[dy * Self::CURSOR_W + dx];
            }
        }
        
        self.cursor_save_x = -100;
        self.cursor_save_y = -100;
    }
}

#[derive(Default)]
pub struct OutputConfig {
    pub make: Option<String>,
    pub model: Option<String>,
    pub physical_size: Option<(i32, i32)>,
    pub resolution: Option<(i32, i32)>,
    pub refresh: Option<i32>,
    pub scale: Option<i32>,
    pub transform: Option<OutputTransform>,
}

pub struct Window {
    pub id: WindowId,
    pub xdg_surface: XdgSurface,
    pub xdg_toplevel: XdgToplevel,
    pub wl_surface: WlSurface,
    pub geometry: Rectangle,
    pub mapped: bool,
    pub buffer: Option<WlBuffer>,
    pub pending_buffer: Option<WlBuffer>,
    pub pending_buffer_set: bool,
    pub needs_redraw: bool,
    pub pixel_cache: Vec<u32>,
    pub cache_width: usize,
    pub cache_height: usize,
    pub cache_stride: usize,
    pub title: String,
    pub workspace: usize,
}

pub type LayerSurfaceId = u64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Background = 0,
    Bottom = 1,
    #[default]
    Top = 2,
    Overlay = 3,
}

pub struct LayerSurface {
    pub id: LayerSurfaceId,
    pub wl_surface: WlSurface,
    pub layer_surface: ZwlrLayerSurfaceV1,
    pub layer: Layer,
    pub namespace: String,
    pub anchor: Anchor,
    pub exclusive_zone: i32,
    pub margin: (i32, i32, i32, i32),
    pub keyboard_interactivity: KeyboardInteractivity,
    pub geometry: Rectangle,
    pub desired_width: u32,
    pub desired_height: u32,
    pub mapped: bool,
    pub buffer: Option<WlBuffer>,
    pub pending_buffer: Option<WlBuffer>,
    pub pending_buffer_set: bool,
    pub needs_redraw: bool,
    pub pixel_cache: Vec<u32>,
    pub cache_width: usize,
    pub cache_height: usize,
    pub cache_stride: usize,
}

pub struct State {
    pub config: Config,
    pub windows: Vec<Window>,
    pub focused_window: Option<WindowId>,
    pub next_window_id: WindowId,
    pub outputs: Vec<Output>,
    pub next_output_id: OutputId,
    pub canvas: Canvas,
    pub gpu_renderer: Option<crate::renderer::GpuRenderer>,

    pub layer_surfaces: Vec<LayerSurface>,
    pub next_layer_surface_id: LayerSurfaceId,

    pub shm_pools: HashMap<ObjectId, ShmPoolData>,
    pub buffers: HashMap<ObjectId, BufferData>,
    pub dmabuf_buffers: HashMap<ObjectId, DmaBufBufferInfo>,

    pub frame_callbacks: Vec<WlCallback>,

    pub keyboards: Vec<WlKeyboard>,
    pub keyboard_to_window: HashMap<ObjectId, WindowId>,
    pub pointers: Vec<WlPointer>,
    pub keyboard_serial: u32,
    pub pointer_serial: u32,
    
    pub pointer_x: f64,
    pub pointer_y: f64,
    pub pointer_focus: Option<WindowId>,
    
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub cursor_visible: bool,
    
    pub keymap_data: Option<KeymapData>,
    
    pub pending_xdg_surfaces: HashMap<u32, (XdgSurface, WlSurface)>,
    
    pub needs_relayout: bool,
    
    pub screencopy_frames: Vec<PendingScreencopy>,

    pub damage_tracker: DamageTracker,
    pub last_cursor_pos: (i32, i32),
    
    pub active_workspace: usize,
    pub workspace_count: usize,
    pub pending_title_change: Option<String>,
}

impl Drop for State {
    fn drop(&mut self) {
        for pool in self.shm_pools.values() {
            if let Some(ptr) = pool.mmap_ptr {
                unsafe {
                    libc::munmap(ptr.as_ptr() as *mut libc::c_void, pool.size as usize);
                }
            }
        }
    }
}

pub struct ShmPoolData {
    pub fd: OwnedFd,
    pub size: i32,
    pub mmap_ptr: Option<NonNull<u8>>,
}

#[derive(Clone)]
pub struct BufferData {
    pub pool_id: ObjectId,
    pub offset: i32,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    #[allow(dead_code)]
    pub format: u32,
}

#[derive(Clone)]
pub struct DmaBufBufferInfo {
    pub width: i32,
    pub height: i32,
    pub format: u32,
    pub modifier: u64,
    pub fd: std::os::fd::RawFd,
    pub stride: u32,
    pub offset: u32,
}

pub struct KeymapData {
    pub fd: OwnedFd,
    pub size: u32,
}

#[derive(Clone)]
pub struct ScreencopyFrameState {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl State {
    pub fn new(config: Config) -> Self {
        let default_width = 1920;
        let default_height = 1080;
        
        let keymap_data = Self::create_keymap(&config);
        let bg_color = config.background_dark();
        
        Self {
            config,
            windows: Vec::new(),
            focused_window: None,
            next_window_id: 1,
            outputs: Vec::new(),
            next_output_id: 1,
            canvas: Canvas::new(default_width, default_height, bg_color),
            gpu_renderer: None,
            layer_surfaces: Vec::new(),
            next_layer_surface_id: 1,
            shm_pools: HashMap::new(),
            buffers: HashMap::new(),
            dmabuf_buffers: HashMap::new(),
            frame_callbacks: Vec::new(),
            keyboards: Vec::new(),
            keyboard_to_window: HashMap::new(),
            pointers: Vec::new(),
            keyboard_serial: 0,
            pointer_serial: 0,
            pointer_x: 0.0,
            pointer_y: 0.0,
            pointer_focus: None,
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: true,
            keymap_data,
            pending_xdg_surfaces: HashMap::new(),
            needs_relayout: false,
            screencopy_frames: Vec::new(),
            damage_tracker: DamageTracker::new(),
            last_cursor_pos: (0, 0),
            active_workspace: 1,
            workspace_count: 4,
            pending_title_change: None,
        }
    }
    
    fn create_keymap(config: &Config) -> Option<KeymapData> {
        use std::io::Write;
        use std::os::fd::FromRawFd;
        
        let xkb_context = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
        let keymap = xkbcommon::xkb::Keymap::new_from_names(
            &xkb_context,
            "",  // rules - use default
            config.keyboard.model.as_str(),
            config.keyboard.layout.as_str(),
            "",  // variant - use default
            if config.keyboard.options.is_empty() {
                None
            } else {
                Some(config.keyboard.options.clone())
            },
            xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
        )?;
        
        let keymap_string = keymap.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
        let keymap_bytes = keymap_string.as_bytes();
        let size = keymap_bytes.len() + 1;
        
        let name = std::ffi::CString::new("ktc-keymap").ok()?;
        let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            log::error!("Failed to create memfd for keymap");
            return None;
        }
        
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        if file.write_all(keymap_bytes).is_err() {
            log::error!("Failed to write keymap to memfd");
            return None;
        }
        if file.write_all(&[0]).is_err() {
            log::error!("Failed to write null terminator to keymap");
            return None;
        }
        
        log::debug!("Created keymap (size={})", size);
        
        Some(KeymapData {
            fd: file.into(),
            size: size as u32,
        })
    }
    
    pub fn add_output(&mut self, name: String, width: i32, height: i32) -> OutputId {
        let id = self.next_output_id;
        self.next_output_id += 1;
        
        let output = Output::new(id, name, width, height);
        self.outputs.push(output);
        
        if self.outputs.len() == 1 {
            let bg_color = self.config.background_dark();
            self.canvas.resize(width as usize, height as usize, bg_color);
        }
        
        self.relayout_windows();
        
        id
    }
    
    pub fn configure_output(&mut self, id: OutputId, config: OutputConfig) {
        let is_primary = self.outputs.first().map(|o| o.id) == Some(id);
        
        let (new_width, new_height) = {
            if let Some(output) = self.outputs.iter_mut().find(|o| o.id == id) {
                if let Some(make) = config.make {
                    output.make = make;
                }
                if let Some(model) = config.model {
                    output.model = model;
                }
                if let Some((w, h)) = config.physical_size {
                    output.physical_width = w;
                    output.physical_height = h;
                }
                if let Some((w, h)) = config.resolution {
                    output.width = w;
                    output.height = h;
                }
                if let Some(refresh) = config.refresh {
                    output.refresh = refresh;
                }
                if let Some(scale) = config.scale {
                    output.scale = scale;
                }
                if let Some(transform) = config.transform {
                    output.transform = transform;
                }
                
                (output.width, output.height)
            } else {
                return;
            }
        };
        
        if is_primary {
            let bg_color = self.config.background_dark();
            self.canvas.resize(new_width as usize, new_height as usize, bg_color);
        }
        
        self.send_output_configuration(id);
    }
    
    fn send_output_configuration(&self, id: OutputId) {
        use wayland_server::protocol::wl_output::{Mode, Subpixel, Transform};
        
        if let Some(output) = self.outputs.iter().find(|o| o.id == id) {
            let transform = match output.transform {
                OutputTransform::Normal => Transform::Normal,
                OutputTransform::Rotate90 => Transform::_90,
                OutputTransform::Rotate180 => Transform::_180,
                OutputTransform::Rotate270 => Transform::_270,
                OutputTransform::Flipped => Transform::Flipped,
                OutputTransform::FlippedRotate90 => Transform::Flipped90,
                OutputTransform::FlippedRotate180 => Transform::Flipped180,
                OutputTransform::FlippedRotate270 => Transform::Flipped270,
            };
            
            for wl_output in &output.wl_outputs {
                wl_output.geometry(
                    output.x,
                    output.y,
                    output.physical_width,
                    output.physical_height,
                    Subpixel::Unknown,
                    output.make.clone(),
                    output.model.clone(),
                    transform,
                );
                wl_output.mode(
                    Mode::Current | Mode::Preferred,
                    output.width,
                    output.height,
                    output.refresh,
                );
                if wl_output.version() >= 2 {
                    wl_output.done();
                }
                if wl_output.version() >= 4 {
                    wl_output.name(output.name.clone());
                }
            }
        }
    }
    
    pub fn register_wl_output(&mut self, wl_output: WlOutput) {
        if let Some(output) = self.outputs.first_mut() {
            output.wl_outputs.push(wl_output);
            let id = output.id;
            self.send_output_configuration(id);
        }
    }
    
    pub fn primary_output(&self) -> Option<&Output> {
        self.outputs.first()
    }
    
    pub fn title_bar_height(&self) -> i32 {
        self.config.title_bar_height()
    }
    
    pub fn screen_size(&self) -> (i32, i32) {
        self.primary_output()
            .map(|o| (o.width, o.height))
            .unwrap_or((1920, 1080))
    }
    
    #[allow(dead_code)]
    pub fn set_screen_size(&mut self, width: i32, height: i32) {
        if self.outputs.is_empty() {
            self.add_output("default".to_string(), width, height);
        } else if let Some(output) = self.outputs.first_mut() {
            output.width = width;
            output.height = height;
            let bg_color = self.config.background_dark();
            self.canvas.resize(width as usize, height as usize, bg_color);
            let id = output.id;
            self.send_output_configuration(id);
        }
        self.damage_tracker.mark_full_damage();
        self.relayout_windows();
    }
    
    pub fn mark_surface_damage(&mut self, surface_id: ObjectId) {
        if let Some(window) = self.windows.iter_mut().find(|w| w.wl_surface.id() == surface_id) {
            window.needs_redraw = true;
            let geometry = window.geometry;
            self.damage_tracker.add_damage(geometry);
        }
    }
    
    pub fn mark_layer_surface_damage(&mut self, surface_id: ObjectId) {
        if let Some(ls) = self.layer_surfaces.iter_mut().find(|ls| ls.wl_surface.id() == surface_id) {
            ls.needs_redraw = true;
            let geometry = ls.geometry;
            self.damage_tracker.add_damage(geometry);
        }
    }
    
    pub fn next_keyboard_serial(&mut self) -> u32 {
        self.keyboard_serial = self.keyboard_serial.wrapping_add(1);
        self.keyboard_serial
    }
    
    pub fn next_pointer_serial(&mut self) -> u32 {
        self.pointer_serial = self.pointer_serial.wrapping_add(1);
        self.pointer_serial
    }
    
    #[allow(dead_code)]
    pub fn add_window(&mut self, xdg_surface: XdgSurface, xdg_toplevel: XdgToplevel, wl_surface: WlSurface) -> WindowId {
        let id = self.add_window_without_relayout(xdg_surface, xdg_toplevel, wl_surface);
        self.relayout_windows();
        id
    }
    
    pub fn add_window_without_relayout(&mut self, xdg_surface: XdgSurface, xdg_toplevel: XdgToplevel, wl_surface: WlSurface) -> WindowId {
        let id = self.next_window_id;
        self.next_window_id += 1;
        
        log::debug!("[window] Adding window {}", id);
        
        let (screen_width, screen_height) = self.screen_size();
        let num_windows = self.windows.len() + 1;
        let geometry = calculate_tiling_geometry(num_windows - 1, num_windows, screen_width, screen_height);
        
        self.windows.push(Window {
            id,
            xdg_surface,
            xdg_toplevel,
            wl_surface,
            geometry,
            mapped: false,
            buffer: None,
            pending_buffer: None,
            pending_buffer_set: false,
            needs_redraw: true,
            pixel_cache: Vec::new(),
            cache_width: 0,
            cache_height: 0,
            cache_stride: 0,
            title: String::new(),
            workspace: self.active_workspace,
        });
        
        self.damage_tracker.mark_full_damage();
        
        id
    }
    
    pub fn relayout_windows(&mut self) {
        let num_windows = self.windows.len();
        if num_windows == 0 {
            return;
        }
        
        let (screen_width, screen_height) = self.screen_size();
        
        for (i, window) in self.windows.iter_mut().enumerate() {
            let new_geometry = calculate_tiling_geometry(i, num_windows, screen_width, screen_height);
            if window.geometry != new_geometry {
                let old_geom = window.geometry;
                window.geometry = new_geometry;
                window.needs_redraw = true;
                
                if old_geom.width != new_geometry.width || old_geom.height != new_geometry.height {
                    window.cache_width = 0;
                    window.cache_height = 0;
                }
            }
        }
        
        self.damage_tracker.mark_full_damage();
        
        for i in 0..num_windows {
            let window_id = self.windows[i].id;
            let geometry = self.windows[i].geometry;
            let xdg_surface = self.windows[i].xdg_surface.clone();
            let xdg_toplevel = self.windows[i].xdg_toplevel.clone();
            
            let states = self.get_toplevel_states(window_id);
            let serial = self.next_keyboard_serial();
            
            let title_bar_height = self.config.title_bar_height();
            let client_height = (geometry.height - title_bar_height).max(1);
            xdg_toplevel.configure(geometry.width, client_height, states);
            xdg_surface.configure(serial);
        }
    }
    
    pub fn get_window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }
    
    pub fn get_window_by_surface(&mut self, surface: &WlSurface) -> Option<&mut Window> {
        let surface_id = surface.id();
        self.windows.iter_mut().find(|w| w.wl_surface.id() == surface_id)
    }
    
    #[allow(dead_code)]
    pub fn get_focused_window(&mut self) -> Option<&mut Window> {
        let focused_id = self.focused_window?;
        self.windows.iter_mut().find(|w| w.id == focused_id)
    }
    
    pub fn get_toplevel_states(&self, window_id: WindowId) -> Vec<u8> {
        let num_windows = self.windows.len();
        let window_index = self.windows.iter().position(|w| w.id == window_id);
        let is_focused = self.focused_window == Some(window_id);
        
        let mut states = vec![];
        
        if is_focused {
            states.extend_from_slice(&(ToplevelState::Activated as u32).to_ne_bytes());
        }
        
        if num_windows >= 2 {
            if num_windows == 2 {
                if window_index == Some(0) {
                    states.extend_from_slice(&(ToplevelState::TiledLeft as u32).to_ne_bytes());
                    states.extend_from_slice(&(ToplevelState::TiledTop as u32).to_ne_bytes());
                    states.extend_from_slice(&(ToplevelState::TiledBottom as u32).to_ne_bytes());
                } else {
                    states.extend_from_slice(&(ToplevelState::TiledRight as u32).to_ne_bytes());
                    states.extend_from_slice(&(ToplevelState::TiledTop as u32).to_ne_bytes());
                    states.extend_from_slice(&(ToplevelState::TiledBottom as u32).to_ne_bytes());
                }
            } else {
                states.extend_from_slice(&(ToplevelState::TiledLeft as u32).to_ne_bytes());
                states.extend_from_slice(&(ToplevelState::TiledRight as u32).to_ne_bytes());
                states.extend_from_slice(&(ToplevelState::TiledTop as u32).to_ne_bytes());
                states.extend_from_slice(&(ToplevelState::TiledBottom as u32).to_ne_bytes());
            }
        }
        
        states
    }
    
    pub fn remove_window(&mut self, id: WindowId) {
        if let Some(pos) = self.windows.iter().position(|w| w.id == id) {
            let geometry = self.windows[pos].geometry;
            self.damage_tracker.add_damage(geometry);
            self.windows.swap_remove(pos);
            log::debug!("[window] Removed window {}", id);
        }
        self.keyboard_to_window.retain(|_, window_id| *window_id != id);
        
        if self.focused_window == Some(id) {
            self.focused_window = None;
            if let Some(first_window) = self.windows.first() {
                let new_focus_id = first_window.id;
                self.set_focus(new_focus_id);
            }
        }
        
        self.damage_tracker.mark_full_damage();
    }
    
    pub fn close_window(&mut self, id: WindowId) {
        if let Some(window) = self.windows.iter().find(|w| w.id == id) {
            window.xdg_toplevel.close();
        }
    }
    
    pub fn focus_next(&mut self) {
        if self.windows.is_empty() {
            return;
        }
        
        let current_idx = self.focused_window
            .and_then(|id| self.windows.iter().position(|w| w.id == id))
            .unwrap_or(0);
        
        let next_idx = (current_idx + 1) % self.windows.len();
        let next_id = self.windows[next_idx].id;
        
        self.set_focus(next_id);
    }
    
    pub fn focus_prev(&mut self) {
        if self.windows.is_empty() {
            return;
        }
        
        let current_idx = self.focused_window
            .and_then(|id| self.windows.iter().position(|w| w.id == id))
            .unwrap_or(0);
        
        let prev_idx = if current_idx == 0 {
            self.windows.len() - 1
        } else {
            current_idx - 1
        };
        let prev_id = self.windows[prev_idx].id;
        
        self.set_focus(prev_id);
    }
    
    pub fn switch_workspace(&mut self, workspace: usize) {
        if workspace < 1 || workspace > self.workspace_count {
            return;
        }
        
        self.active_workspace = workspace;
        
        let first_window = self.windows.iter()
            .find(|w| w.workspace == workspace && w.mapped)
            .map(|w| w.id);
        
        if let Some(id) = first_window {
            self.set_focus(id);
        } else {
            self.focused_window = None;
        }
        
        self.needs_relayout = true;
        self.damage_tracker.mark_full_damage();
        log::info!("Switched to workspace {}", workspace);
    }
    
    pub fn move_window_to_workspace(&mut self, window_id: WindowId, workspace: usize) {
        if workspace < 1 || workspace > self.workspace_count {
            return;
        }
        
        if let Some(window) = self.windows.iter_mut().find(|w| w.id == window_id) {
            window.workspace = workspace;
            self.needs_relayout = true;
            self.damage_tracker.mark_full_damage();
        }
    }
    
    pub fn set_window_title(&mut self, window_id: WindowId, title: String) {
        if let Some(window) = self.windows.iter_mut().find(|w| w.id == window_id) {
            window.title = title;
        }
    }
    
    pub fn set_focus(&mut self, window_id: WindowId) {
        self.set_focus_without_relayout(window_id);
    }
    
    #[allow(dead_code)]
    fn send_configure_to_window(&mut self, window_id: WindowId) {
        if let Some(window) = self.windows.iter().find(|w| w.id == window_id) {
            let geometry = window.geometry;
            let xdg_surface = window.xdg_surface.clone();
            let xdg_toplevel = window.xdg_toplevel.clone();
            let states = self.get_toplevel_states(window_id);
            let serial = self.next_keyboard_serial();
            
            xdg_toplevel.configure(geometry.width, geometry.height, states);
            xdg_surface.configure(serial);
        }
    }
    
    pub fn set_focus_without_relayout(&mut self, window_id: WindowId) {
        let old_focused = self.focused_window;
        
        if old_focused == Some(window_id) {
            return;
        }
        
        if let Some(old_id) = old_focused {
            if let Some(old_win) = self.windows.iter_mut().find(|w| w.id == old_id) {
                old_win.needs_redraw = true;
                self.damage_tracker.add_damage(old_win.geometry);
            }
        }
        
        self.focused_window = Some(window_id);
        
        if let Some(new_win) = self.windows.iter_mut().find(|w| w.id == window_id) {
            new_win.needs_redraw = true;
            self.damage_tracker.add_damage(new_win.geometry);
        }
        
        if let Some(old_id) = old_focused {
            if let Some(old_window) = self.windows.iter().find(|w| w.id == old_id) {
                let old_surface = old_window.wl_surface.clone();
                let old_client = old_window.wl_surface.client();
                let serial = self.next_keyboard_serial();
                
                for keyboard in self.keyboards.iter() {
                    if keyboard.client() == old_client {
                        keyboard.leave(serial, &old_surface);
                    }
                }
            }
        }
        
        let new_window_info = self.windows.iter()
            .find(|w| w.id == window_id)
            .map(|w| (w.wl_surface.clone(), w.wl_surface.client()));
        
        if let Some((surface, Some(new_client))) = new_window_info {
            let serial = self.next_keyboard_serial();
            
            for keyboard in self.keyboards.iter() {
                if keyboard.client().as_ref() == Some(&new_client) {
                    keyboard.enter(serial, &surface, vec![]);
                    self.keyboard_to_window.insert(keyboard.id(), window_id);
                }
            }
        }
    }
    
    pub fn add_shm_pool(&mut self, pool: &WlShmPool, fd: OwnedFd, size: i32) {
        let id = pool.id();
        self.shm_pools.insert(id, ShmPoolData { fd, size, mmap_ptr: None });
    }
    
    pub fn resize_shm_pool(&mut self, pool: &WlShmPool, new_size: i32) {
        let id = pool.id();
        if let Some(pool_data) = self.shm_pools.get_mut(&id) {
            if new_size > pool_data.size {
                if let Some(old_ptr) = pool_data.mmap_ptr.take() {
                    unsafe {
                        libc::munmap(old_ptr.as_ptr() as *mut libc::c_void, pool_data.size as usize);
                    }
                }
                pool_data.size = new_size;
                log::debug!("[shm] Pool {:?} resized to {} bytes", id, new_size);
            }
        }
    }
    
    #[allow(clippy::too_many_arguments)]
    pub fn add_buffer(&mut self, buffer: &WlBuffer, pool: &WlShmPool, offset: i32, 
                      width: i32, height: i32, stride: i32, format: u32) {
        let buffer_id = buffer.id();
        let pool_id = pool.id();
        self.buffers.insert(buffer_id, BufferData {
            pool_id,
            offset,
            width,
            height,
            stride,
            format,
        });
    }
    
    #[allow(dead_code)]
    pub fn get_buffer_pixels(&mut self, buffer: &WlBuffer) -> Option<(&[u32], usize)> {
        let buffer_id = buffer.id();
        let buffer_data = self.buffers.get(&buffer_id)?;
        let pool_id = buffer_data.pool_id.clone();
        let offset = buffer_data.offset;
        let height = buffer_data.height;
        let stride = buffer_data.stride;
        
        let pool_data = self.shm_pools.get_mut(&pool_id)?;
        
        if pool_data.mmap_ptr.is_none() {
            unsafe {
                let ptr = libc::mmap(
                    std::ptr::null_mut(),
                    pool_data.size as usize,
                    libc::PROT_READ,
                    libc::MAP_SHARED,
                    pool_data.fd.as_fd().as_raw_fd(),
                    0,
                );
                
                if ptr == libc::MAP_FAILED {
                    return None;
                }
                
                pool_data.mmap_ptr = NonNull::new(ptr as *mut u8);
            }
        }
        
        let mmap_ptr = pool_data.mmap_ptr?;
        let stride_pixels = (stride / 4) as usize;
        
        unsafe {
            let buffer_start = mmap_ptr.as_ptr().add(offset as usize) as *const u32;
            let pixel_count = stride_pixels * height as usize;
            
            Some((std::slice::from_raw_parts(buffer_start, pixel_count), stride_pixels))
        }
    }
    
    pub fn update_window_pixel_cache(&mut self, window_id: WindowId) -> bool {
        let (buffer_id, buf_width, buf_height, expected_width, expected_height) = {
            let window = match self.windows.iter().find(|w| w.id == window_id) {
                Some(w) => w,
                None => return false,
            };
            let buffer = match &window.buffer {
                Some(b) => b,
                None => return false,
            };
            let buffer_id = buffer.id();
            let buffer_data = match self.buffers.get(&buffer_id) {
                Some(d) => d,
                None => return false,
            };
            let expected_w = window.geometry.width;
            let title_bar_height = self.config.title_bar_height();
            let expected_h = (window.geometry.height - title_bar_height).max(1);
            (buffer_id, buffer_data.width as usize, buffer_data.height as usize, expected_w, expected_h)
        };
        
        let min_width = (expected_width / 2).max(10) as usize;
        let min_height = (expected_height / 2).max(10) as usize;
        
        if buf_width < min_width || buf_height < min_height {
            return false;
        }
        
        let buffer_data = match self.buffers.get(&buffer_id) {
            Some(d) => d.clone(),
            None => return false,
        };
        
        let pool_data = match self.shm_pools.get_mut(&buffer_data.pool_id) {
            Some(p) => p,
            None => return false,
        };
        
        if pool_data.mmap_ptr.is_none() {
            unsafe {
                let ptr = libc::mmap(
                    std::ptr::null_mut(),
                    pool_data.size as usize,
                    libc::PROT_READ,
                    libc::MAP_SHARED,
                    pool_data.fd.as_fd().as_raw_fd(),
                    0,
                );
                
                if ptr == libc::MAP_FAILED {
                    return false;
                }
                
                pool_data.mmap_ptr = NonNull::new(ptr as *mut u8);
            }
        }
        
        let mmap_ptr = match pool_data.mmap_ptr {
            Some(p) => p,
            None => return false,
        };
        
        let stride_pixels = (buffer_data.stride / 4) as usize;
        let pixel_count = stride_pixels * buf_height;
        let byte_count = pixel_count * 4;
        let end_offset = buffer_data.offset as usize + byte_count;
        
        if end_offset > pool_data.size as usize {
            log::warn!(
                "[cache] Buffer exceeds pool bounds: offset={} + size={} > pool_size={}",
                buffer_data.offset, byte_count, pool_data.size
            );
            return false;
        }
        
        let window = match self.windows.iter_mut().find(|w| w.id == window_id) {
            Some(w) => w,
            None => return false,
        };
        
        if window.pixel_cache.len() < pixel_count {
            window.pixel_cache.resize(pixel_count, 0);
        }
        
        unsafe {
            let src = mmap_ptr.as_ptr().add(buffer_data.offset as usize) as *const u32;
            std::ptr::copy_nonoverlapping(src, window.pixel_cache.as_mut_ptr(), pixel_count);
        }
        
        window.cache_width = buf_width;
        window.cache_height = buf_height;
        window.cache_stride = stride_pixels;
        
        true
    }
    
    pub fn update_layer_surface_pixel_cache(&mut self, layer_surface_id: LayerSurfaceId) -> bool {
        let (buffer_id, buf_width, buf_height) = {
            let ls = match self.layer_surfaces.iter().find(|ls| ls.id == layer_surface_id) {
                Some(ls) => ls,
                None => return false,
            };
            let buffer = match &ls.buffer {
                Some(b) => b,
                None => return false,
            };
            let buffer_id = buffer.id();
            let buffer_data = match self.buffers.get(&buffer_id) {
                Some(d) => d,
                None => return false,
            };
            (buffer_id, buffer_data.width as usize, buffer_data.height as usize)
        };
        
        let buffer_data = match self.buffers.get(&buffer_id) {
            Some(d) => d.clone(),
            None => return false,
        };
        
        let pool_data = match self.shm_pools.get_mut(&buffer_data.pool_id) {
            Some(p) => p,
            None => return false,
        };
        
        if pool_data.mmap_ptr.is_none() {
            unsafe {
                let ptr = libc::mmap(
                    std::ptr::null_mut(),
                    pool_data.size as usize,
                    libc::PROT_READ,
                    libc::MAP_SHARED,
                    pool_data.fd.as_fd().as_raw_fd(),
                    0,
                );
                
                if ptr == libc::MAP_FAILED {
                    return false;
                }
                
                pool_data.mmap_ptr = NonNull::new(ptr as *mut u8);
            }
        }
        
        let mmap_ptr = match pool_data.mmap_ptr {
            Some(p) => p,
            None => return false,
        };
        
        let stride_pixels = (buffer_data.stride / 4) as usize;
        let pixel_count = stride_pixels * buf_height;
        let byte_count = pixel_count * 4;
        let end_offset = buffer_data.offset as usize + byte_count;
        
        if end_offset > pool_data.size as usize {
            log::warn!(
                "[cache] Layer surface buffer exceeds pool bounds: offset={} + size={} > pool_size={}",
                buffer_data.offset, byte_count, pool_data.size
            );
            return false;
        }
        
        let ls = match self.layer_surfaces.iter_mut().find(|ls| ls.id == layer_surface_id) {
            Some(ls) => ls,
            None => return false,
        };
        
        if ls.pixel_cache.len() < pixel_count {
            ls.pixel_cache.resize(pixel_count, 0);
        }
        
        unsafe {
            let src = mmap_ptr.as_ptr().add(buffer_data.offset as usize) as *const u32;
            std::ptr::copy_nonoverlapping(src, ls.pixel_cache.as_mut_ptr(), pixel_count);
        }
        
        ls.cache_width = buf_width;
        ls.cache_height = buf_height;
        ls.cache_stride = stride_pixels;
        
        true
    }
    
    pub fn get_focused_keyboards(&self) -> Vec<WlKeyboard> {
        let focused_id = match self.focused_window {
            Some(id) => id,
            None => return vec![],
        };
        
        let focused_client = self.windows.iter()
            .find(|w| w.id == focused_id)
            .and_then(|w| w.wl_surface.client());
        
        let focused_client = match focused_client {
            Some(c) => c,
            None => return vec![],
        };
        
        self.keyboards.iter()
            .filter(|kb| kb.client().as_ref() == Some(&focused_client))
            .cloned()
            .collect()
    }
    
    pub fn window_at(&self, x: f64, y: f64) -> Option<WindowId> {
        let title_bar_height = self.config.title_bar_height();
        for window in self.windows.iter().rev() {
            if !window.mapped {
                continue;
            }
            let g = window.geometry;
            let content_y = g.y + title_bar_height;
            if x >= g.x as f64 && x < (g.x + g.width) as f64 &&
               y >= g.y as f64 && y < (content_y + g.height - title_bar_height) as f64 {
                return Some(window.id);
            }
        }
        None
    }
    
    pub fn handle_pointer_motion(&mut self, x: f64, y: f64) {
        let old_x = self.cursor_x;
        let old_y = self.cursor_y;
        self.cursor_x = x as i32;
        self.cursor_y = y as i32;
        self.pointer_x = x;
        self.pointer_y = y;
        
        if self.cursor_visible && (old_x != self.cursor_x || old_y != self.cursor_y) {
            self.last_cursor_pos = (old_x, old_y);
            self.damage_tracker.add_cursor_damage();
        }
        
        let window_id = self.window_at(x, y);
        let title_bar_height = self.config.title_bar_height();
        
        if window_id != self.pointer_focus {
            let serial = self.next_pointer_serial();
            
            if let Some(old_id) = self.pointer_focus {
                if let Some(old_window) = self.windows.iter().find(|w| w.id == old_id) {
                    let old_client = old_window.wl_surface.client();
                    for pointer in &self.pointers {
                        if pointer.client() == old_client {
                            pointer.leave(serial, &old_window.wl_surface);
                        }
                    }
                }
            }
            
            if let Some(new_id) = window_id {
                if let Some(new_window) = self.windows.iter().find(|w| w.id == new_id) {
                    let new_client = new_window.wl_surface.client();
                    let g = new_window.geometry;
                    let local_x = x - g.x as f64;
                    let local_y = y - (g.y + title_bar_height) as f64;
                    
                    for pointer in &self.pointers {
                        if pointer.client() == new_client {
                            pointer.enter(serial, &new_window.wl_surface, local_x, local_y);
                        }
                    }
                }
            }
            
            self.pointer_focus = window_id;
        } else if let Some(win_id) = window_id {
            if let Some(window) = self.windows.iter().find(|w| w.id == win_id) {
                let client = window.wl_surface.client();
                let g = window.geometry;
                let local_x = x - g.x as f64;
                let local_y = y - (g.y + title_bar_height) as f64;
                let time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u32;
                
                for pointer in &self.pointers {
                    if pointer.client() == client {
                        pointer.motion(time, local_x, local_y);
                    }
                }
            }
        }
    }
    
    pub fn handle_pointer_button(&mut self, button: u32, pressed: bool) {
        let state = if pressed {
            wayland_server::protocol::wl_pointer::ButtonState::Pressed
        } else {
            wayland_server::protocol::wl_pointer::ButtonState::Released
        };
        
        let serial = self.next_pointer_serial();
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u32;
        
        if pressed {
            if let Some(win_id) = self.pointer_focus {
                if self.focused_window != Some(win_id) {
                    self.set_focus(win_id);
                }
            }
        }
        
        if let Some(win_id) = self.pointer_focus {
            if let Some(window) = self.windows.iter().find(|w| w.id == win_id) {
                let client = window.wl_surface.client();
                for pointer in &self.pointers {
                    if pointer.client() == client {
                        pointer.button(serial, time, button, state);
                    }
                }
            }
        }
    }
    
    pub fn handle_pointer_axis(&mut self, horizontal: f64, vertical: f64) {
        use wayland_server::protocol::wl_pointer::Axis;
        
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u32;
        
        if let Some(win_id) = self.pointer_focus {
            if let Some(window) = self.windows.iter().find(|w| w.id == win_id) {
                let client = window.wl_surface.client();
                for pointer in &self.pointers {
                    if pointer.client() == client {
                        if vertical.abs() > 0.0 {
                            pointer.axis(time, Axis::VerticalScroll, vertical);
                        }
                        if horizontal.abs() > 0.0 {
                            pointer.axis(time, Axis::HorizontalScroll, horizontal);
                        }
                        if pointer.version() >= 5 {
                            pointer.frame();
                        }
                    }
                }
            }
        }
    }
}

fn calculate_tiling_geometry(index: usize, num_windows: usize, screen_width: i32, screen_height: i32) -> Rectangle {
    if num_windows == 0 {
        return Rectangle { x: 0, y: 0, width: screen_width, height: screen_height };
    }
    
    if num_windows == 1 {
        return Rectangle {
            x: 0,
            y: 0,
            width: screen_width,
            height: screen_height,
        };
    }
    
    if num_windows == 2 {
        let half = screen_width / 2;
        if index == 0 {
            Rectangle {
                x: 0,
                y: 0,
                width: half,
                height: screen_height,
            }
        } else {
            Rectangle {
                x: half,
                y: 0,
                width: screen_width - half,
                height: screen_height,
            }
        }
    } else {
        let cols = (num_windows as f32).sqrt().ceil() as i32;
        let rows = ((num_windows as i32) + cols - 1) / cols;
        let col = (index as i32) % cols;
        let row = (index as i32) / cols;
        
        let base_width = screen_width / cols;
        let base_height = screen_height / rows;
        let extra_width = screen_width % cols;
        let extra_height = screen_height % rows;
        
        let width = base_width + if col < extra_width { 1 } else { 0 };
        let height = base_height + if row < extra_height { 1 } else { 0 };
        
        let x = col * base_width + col.min(extra_width);
        let y = row * base_height + row.min(extra_height);
        
        let width = width.max(100);
        let height = height.max(100);
        
        Rectangle { x, y, width, height }
    }
}
