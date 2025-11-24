use std::collections::HashMap;
use std::os::fd::{OwnedFd, AsFd, AsRawFd};
use wayland_server::protocol::{wl_surface::WlSurface, wl_buffer::WlBuffer, wl_shm_pool::WlShmPool, wl_callback::WlCallback};
use wayland_server::Resource;

pub struct State {
    pub surfaces: HashMap<u32, SurfaceData>,
    pub shm_pools: HashMap<u32, ShmPoolData>,
    pub buffers: HashMap<u32, BufferData>,
    pub frame_callbacks: Vec<WlCallback>,
}

#[derive(Default)]
pub struct SurfaceData {
    pub buffer: Option<WlBuffer>,
    pub pending_buffer: Option<WlBuffer>,
    pub size: (i32, i32),
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
            surfaces: HashMap::new(),
            shm_pools: HashMap::new(),
            buffers: HashMap::new(),
            frame_callbacks: Vec::new(),
        }
    }
    
    pub fn get_surface_data(&mut self, surface: &WlSurface) -> &mut SurfaceData {
        let id = surface.id().protocol_id();
        self.surfaces.entry(id).or_insert_with(SurfaceData::default)
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
    
    pub fn get_buffer_pixels(&self, buffer: &WlBuffer) -> Option<Vec<u32>> {
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
            
            let buffer_start = (ptr as *const u8).add(buffer_data.offset as usize);
            let pixel_count = (buffer_data.width * buffer_data.height) as usize;
            let mut pixels = Vec::with_capacity(pixel_count);
            
            for y in 0..buffer_data.height {
                let row_start = buffer_start.add((y * buffer_data.stride) as usize) as *const u32;
                for x in 0..buffer_data.width {
                    pixels.push(*row_start.add(x as usize));
                }
            }
            
            libc::munmap(ptr, pool_data.size as usize);
            
            Some(pixels)
        }
    }
}

