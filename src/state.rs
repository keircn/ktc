use std::collections::HashMap;
use std::os::fd::{OwnedFd, AsFd, AsRawFd};
use std::ptr::NonNull;
use wayland_server::protocol::{wl_surface::WlSurface, wl_buffer::WlBuffer, wl_shm_pool::WlShmPool, wl_callback::WlCallback, wl_keyboard::WlKeyboard, wl_pointer::WlPointer};
use wayland_server::Resource;
use wayland_protocols::xdg::shell::server::{xdg_surface::XdgSurface, xdg_toplevel::{XdgToplevel, State as ToplevelState}};

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
    
    pub screen_width: i32,
    pub screen_height: i32,
    
    pub shm_pools: HashMap<u32, ShmPoolData>,
    pub buffers: HashMap<u32, BufferData>,
    pub frame_callbacks: Vec<WlCallback>,
    
    pub keyboards: Vec<WlKeyboard>,
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
        Self {
            windows: Vec::new(),
            focused_window: None,
            next_window_id: 1,
            screen_width: 1920,
            screen_height: 1080,
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
    
    pub fn set_screen_size(&mut self, width: i32, height: i32) {
        self.screen_width = width;
        self.screen_height = height;
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
        
        let num_windows = self.windows.len();
        let geometry = calculate_tiling_geometry(num_windows, self.screen_width, self.screen_height);
        
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
        
        let screen_width = self.screen_width;
        let screen_height = self.screen_height;
        
        for (i, window) in self.windows.iter_mut().enumerate() {
            window.geometry = calculate_tiling_geometry(i, screen_width, screen_height);
        }
        
        for i in 0..num_windows {
            let window_id = self.windows[i].id;
            let geometry = self.windows[i].geometry;
            let xdg_surface = self.windows[i].xdg_surface.clone();
            let xdg_toplevel = self.windows[i].xdg_toplevel.clone();
            
            let states = self.get_tiling_states_for_window(window_id);
            let serial = self.next_keyboard_serial();
            
            xdg_surface.configure(serial);
            xdg_toplevel.configure(geometry.width, geometry.height, states);
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
    
    pub fn get_tiling_states_for_window(&self, window_id: WindowId) -> Vec<u8> {
        let num_windows = self.windows.len();
        let window_index = self.windows.iter().position(|w| w.id == window_id);
        
        if num_windows == 1 {
            return vec![];
        }
        
        let mut states = vec![];
        
        if num_windows == 2 {
            if window_index == Some(0) {
                states.push(ToplevelState::TiledLeft as u8);
            } else {
                states.push(ToplevelState::TiledRight as u8);
            }
            states.push(ToplevelState::TiledTop as u8);
            states.push(ToplevelState::TiledBottom as u8);
        } else {
            states.push(ToplevelState::TiledLeft as u8);
            states.push(ToplevelState::TiledRight as u8);
            states.push(ToplevelState::TiledTop as u8);
            states.push(ToplevelState::TiledBottom as u8);
        }
        
        states
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
        let rows = (num_windows as i32 + cols - 1) / cols;
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
