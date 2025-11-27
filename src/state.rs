use std::collections::HashMap;
use std::os::fd::{OwnedFd, AsFd, AsRawFd};
use std::ptr::NonNull;
use wayland_server::protocol::{wl_surface::WlSurface, wl_buffer::WlBuffer, wl_shm_pool::WlShmPool, wl_callback::WlCallback, wl_keyboard::WlKeyboard, wl_pointer::WlPointer, wl_output::WlOutput};
use wayland_server::Resource;
use wayland_protocols::xdg::shell::server::{xdg_surface::XdgSurface, xdg_toplevel::{XdgToplevel, State as ToplevelState}};
use wayland_server::backend::ObjectId;

pub type WindowId = u64;
pub type OutputId = u64;

#[derive(Clone, Copy, Debug)]
pub struct Rectangle {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
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

    pub fn usable_area(&self) -> Rectangle {
        Rectangle {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }

    pub fn scaled_size(&self) -> (i32, i32) {
        (self.width / self.scale, self.height / self.scale)
    }
}

pub struct Canvas {
    pub pixels: Vec<u32>,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        let stride = width;
        let pixels = vec![0xFF1A1A2E; width * height];
        Self {
            pixels,
            width,
            height,
            stride,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.stride = width;
            self.pixels = vec![0xFF1A1A2E; width * height];
        }
    }

    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }
    
    pub fn clear_with_pattern(&mut self) {
        let bg_dark = 0xFF1A1A2E;
        let bg_light = 0xFF16213E;
        let tile_size = 32;
        
        for y in 0..self.height {
            for x in 0..self.width {
                let tx = x / tile_size;
                let ty = y / tile_size;
                let color = if (tx + ty) % 2 == 0 { bg_dark } else { bg_light };
                self.pixels[y * self.stride + x] = color;
            }
        }
    }

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

    pub fn blit_fast(&mut self, src: &[u32], src_width: usize, src_height: usize, dst_x: i32, dst_y: i32) {
        let dst_x = dst_x.max(0) as usize;
        let dst_y = dst_y.max(0) as usize;

        for y in 0..src_height.min(self.height.saturating_sub(dst_y)) {
            let dst_row = dst_y + y;
            let src_offset = y * src_width;
            let dst_offset = dst_row * self.stride + dst_x;
            let copy_width = src_width.min(self.width.saturating_sub(dst_x));

            if src_offset + copy_width <= src.len() && dst_offset + copy_width <= self.pixels.len() {
                self.pixels[dst_offset..dst_offset + copy_width]
                    .copy_from_slice(&src[src_offset..src_offset + copy_width]);
            }
        }
    }

    pub fn as_slice(&self) -> &[u32] {
        &self.pixels
    }

    pub fn as_mut_slice(&mut self) -> &mut [u32] {
        &mut self.pixels
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
}

pub struct State {
    pub windows: Vec<Window>,
    pub focused_window: Option<WindowId>,
    pub next_window_id: WindowId,
    pub outputs: Vec<Output>,
    pub next_output_id: OutputId,
    pub canvas: Canvas,

    pub shm_pools: HashMap<u32, ShmPoolData>,
    pub buffers: HashMap<u32, BufferData>,

    pub frame_callbacks: Vec<WlCallback>,

    pub keyboards: Vec<WlKeyboard>,
    pub keyboard_to_window: HashMap<ObjectId, WindowId>,
    pub pointers: Vec<WlPointer>,
    pub keyboard_serial: u32,
    pub pointer_serial: u32,
    
    pub pending_xdg_surfaces: HashMap<u32, (XdgSurface, WlSurface)>,
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

pub struct BufferData {
    pub pool_id: u32,
    pub offset: i32,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub format: u32,
}

impl State {
    pub fn new() -> Self {
        let default_width = 1920;
        let default_height = 1080;
        
        Self {
            windows: Vec::new(),
            focused_window: None,
            next_window_id: 1,
            outputs: Vec::new(),
            next_output_id: 1,
            canvas: Canvas::new(default_width, default_height),
            shm_pools: HashMap::new(),
            buffers: HashMap::new(),
            frame_callbacks: Vec::new(),
            keyboards: Vec::new(),
            keyboard_to_window: HashMap::new(),
            pointers: Vec::new(),
            keyboard_serial: 0,
            pointer_serial: 0,
            pending_xdg_surfaces: HashMap::new(),
        }
    }
    
    pub fn add_output(&mut self, name: String, width: i32, height: i32) -> OutputId {
        let id = self.next_output_id;
        self.next_output_id += 1;
        
        let output = Output::new(id, name, width, height);
        self.outputs.push(output);
        
        if self.outputs.len() == 1 {
            self.canvas.resize(width as usize, height as usize);
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
            self.canvas.resize(new_width as usize, new_height as usize);
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
    
    pub fn screen_size(&self) -> (i32, i32) {
        self.primary_output()
            .map(|o| (o.width, o.height))
            .unwrap_or((1920, 1080))
    }
    
    pub fn set_screen_size(&mut self, width: i32, height: i32) {
        if self.outputs.is_empty() {
            self.add_output("default".to_string(), width, height);
        } else if let Some(output) = self.outputs.first_mut() {
            output.width = width;
            output.height = height;
            self.canvas.resize(width as usize, height as usize);
            let id = output.id;
            self.send_output_configuration(id);
        }
        self.relayout_windows();
    }
    
    pub fn next_keyboard_serial(&mut self) -> u32 {
        self.keyboard_serial = self.keyboard_serial.wrapping_add(1);
        self.keyboard_serial
    }
    
    pub fn next_pointer_serial(&mut self) -> u32 {
        self.pointer_serial = self.pointer_serial.wrapping_add(1);
        self.pointer_serial
    }
    
    pub fn add_window(&mut self, xdg_surface: XdgSurface, xdg_toplevel: XdgToplevel, wl_surface: WlSurface) -> WindowId {
        let id = self.next_window_id;
        self.next_window_id += 1;
        
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
        });
        
        self.relayout_windows();
        
        id
    }
    
    pub fn relayout_windows(&mut self) {
        let num_windows = self.windows.len();
        if num_windows == 0 {
            return;
        }
        
        let (screen_width, screen_height) = self.screen_size();
        
        for (i, window) in self.windows.iter_mut().enumerate() {
            window.geometry = calculate_tiling_geometry(i, num_windows, screen_width, screen_height);
        }
        
        for i in 0..num_windows {
            let window_id = self.windows[i].id;
            let geometry = self.windows[i].geometry;
            let xdg_surface = self.windows[i].xdg_surface.clone();
            let xdg_toplevel = self.windows[i].xdg_toplevel.clone();
            
            let states = self.get_toplevel_states(window_id);
            let serial = self.next_keyboard_serial();
            
            xdg_toplevel.configure(geometry.width, geometry.height, states);
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
            self.windows.swap_remove(pos);
        }
        self.keyboard_to_window.retain(|_, window_id| *window_id != id);
        
        if self.focused_window == Some(id) {
            self.focused_window = None;
            if let Some(first_window) = self.windows.first() {
                let new_focus_id = first_window.id;
                self.set_focus(new_focus_id);
            }
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
    
    pub fn set_focus(&mut self, window_id: WindowId) {
        let old_focused = self.focused_window;
        
        if old_focused == Some(window_id) {
            return;
        }
        
        self.focused_window = Some(window_id);
        
        let new_surface = self.windows.iter()
            .find(|w| w.id == window_id)
            .map(|w| w.wl_surface.clone());
        
        if let Some(surface) = new_surface {
            let serial = self.next_keyboard_serial();
            for keyboard in &self.keyboards {
                let kb_id = keyboard.id();
                if let Some(old_id) = old_focused {
                    if self.keyboard_to_window.get(&kb_id) == Some(&old_id) {
                        if let Some(old_window) = self.windows.iter().find(|w| w.id == old_id) {
                            keyboard.leave(serial, &old_window.wl_surface);
                        }
                    }
                }
                
                keyboard.enter(serial, &surface, vec![]);
                self.keyboard_to_window.insert(kb_id, window_id);
            }
        }
        
        self.relayout_windows();
        
        log::info!("Focus changed to window {}", window_id);
    }
    
    pub fn add_shm_pool(&mut self, pool: &WlShmPool, fd: OwnedFd, size: i32) {
        let id = pool.id().protocol_id();
        self.shm_pools.insert(id, ShmPoolData { fd, size, mmap_ptr: None });
    }
    
    pub fn add_buffer(&mut self, buffer: &WlBuffer, pool: &WlShmPool, offset: i32, 
                      width: i32, height: i32, stride: i32, format: u32) {
        let buffer_id = buffer.id().protocol_id();
        let pool_id = pool.id().protocol_id();
        self.buffers.insert(buffer_id, BufferData {
            pool_id,
            offset,
            width,
            height,
            stride,
            format,
        });
    }
    
    pub fn get_buffer_pixels(&mut self, buffer: &WlBuffer) -> Option<&[u32]> {
        let buffer_id = buffer.id().protocol_id();
        let buffer_data = self.buffers.get(&buffer_id)?;
        let pool_id = buffer_data.pool_id;
        let offset = buffer_data.offset;
        let width = buffer_data.width;
        let height = buffer_data.height;
        
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
        
        unsafe {
            let buffer_start = mmap_ptr.as_ptr().add(offset as usize) as *const u32;
            let pixel_count = (width * height) as usize;
            
            Some(std::slice::from_raw_parts(buffer_start, pixel_count))
        }
    }
    
    pub fn get_focused_keyboards(&self) -> Vec<WlKeyboard> {
        let focused_id = match self.focused_window {
            Some(id) => id,
            None => return vec![],
        };
        
        self.keyboards.iter()
            .filter(|kb| {
                let kb_id = kb.id();
                self.keyboard_to_window.get(&kb_id) == Some(&focused_id)
            })
            .cloned()
            .collect()
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
