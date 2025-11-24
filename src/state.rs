use std::collections::HashMap;
use std::os::fd::{OwnedFd, AsFd, AsRawFd};
use wayland_server::protocol::{wl_surface::WlSurface, wl_buffer::WlBuffer, wl_shm_pool::WlShmPool, wl_callback::WlCallback, wl_keyboard::WlKeyboard, wl_pointer::WlPointer};
use wayland_server::Resource;
use wayland_protocols::xdg::shell::server::{xdg_surface::XdgSurface, xdg_toplevel::XdgToplevel};

pub type WindowId = u64;

#[derive(Clone, Copy)]
pub struct Rectangle {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
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
    
    pub shm_pools: HashMap<u32, ShmPoolData>,
    pub buffers: HashMap<u32, BufferData>,
    pub frame_callbacks: Vec<WlCallback>,
    
    pub keyboards: Vec<WlKeyboard>,
    pub pointers: Vec<WlPointer>,
    pub keyboard_serial: u32,
    pub pointer_serial: u32,
    
    pub pending_xdg_surfaces: HashMap<u32, (XdgSurface, WlSurface)>,
}

pub struct ShmPoolData {
    pub fd: OwnedFd,
    pub size: i32,
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
        Self {
            windows: Vec::new(),
            focused_window: None,
            next_window_id: 1,
            shm_pools: HashMap::new(),
            buffers: HashMap::new(),
            frame_callbacks: Vec::new(),
            keyboards: Vec::new(),
            pointers: Vec::new(),
            keyboard_serial: 0,
            pointer_serial: 0,
            pending_xdg_surfaces: HashMap::new(),
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
    
    pub fn add_window(&mut self, xdg_surface: XdgSurface, xdg_toplevel: XdgToplevel, wl_surface: WlSurface, screen_width: i32, screen_height: i32) -> WindowId {
        let id = self.next_window_id;
        self.next_window_id += 1;
        
        let num_windows = self.windows.len();
        let geometry = calculate_tiling_geometry(num_windows, screen_width, screen_height);
        
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
        
        self.relayout_windows(screen_width, screen_height);
        
        id
    }
    
    pub fn relayout_windows(&mut self, screen_width: i32, screen_height: i32) {
        let num_windows = self.windows.len();
        if num_windows == 0 {
            return;
        }
        
        for (i, window) in self.windows.iter_mut().enumerate() {
            window.geometry = calculate_tiling_geometry(i, screen_width, screen_height);
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
    
    pub fn remove_window(&mut self, id: WindowId) {
        if let Some(pos) = self.windows.iter().position(|w| w.id == id) {
            self.windows.swap_remove(pos);
        }
        if self.focused_window == Some(id) {
            self.focused_window = self.windows.first().map(|w| w.id);
        }
    }
    
    pub fn add_shm_pool(&mut self, pool: &WlShmPool, fd: OwnedFd, size: i32) {
        let id = pool.id().protocol_id();
        self.shm_pools.insert(id, ShmPoolData { fd, size });
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
    
    pub fn get_buffer_pixels(&self, buffer: &WlBuffer) -> Option<&[u32]> {
        let buffer_id = buffer.id().protocol_id();
        let buffer_data = self.buffers.get(&buffer_id)?;
        let pool_data = self.shm_pools.get(&buffer_data.pool_id)?;
        
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
            
            let buffer_start = (ptr as *const u8).add(buffer_data.offset as usize) as *const u32;
            let pixel_count = (buffer_data.width * buffer_data.height) as usize;
            
            Some(std::slice::from_raw_parts(buffer_start, pixel_count))
        }
    }
}

fn calculate_tiling_geometry(index: usize, screen_width: i32, screen_height: i32) -> Rectangle {
    let num_windows = index + 1;
    
    if num_windows == 1 {
        return Rectangle {
            x: 0,
            y: 0,
            width: screen_width,
            height: screen_height,
        };
    }
    
    if num_windows == 2 {
        if index == 0 {
            Rectangle {
                x: 0,
                y: 0,
                width: screen_width / 2,
                height: screen_height,
            }
        } else {
            Rectangle {
                x: screen_width / 2,
                y: 0,
                width: screen_width / 2,
                height: screen_height,
            }
        }
    } else {
        let cols = (num_windows as f32).sqrt().ceil() as i32;
        let rows = (num_windows as i32 + cols - 1) / cols;
        let col = (index as i32) % cols;
        let row = (index as i32) / cols;
        let width = screen_width / cols;
        let height = screen_height / rows;
        
        Rectangle {
            x: col * width,
            y: row * height,
            width,
            height,
        }
    }
}
