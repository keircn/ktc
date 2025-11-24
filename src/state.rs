use std::collections::HashMap;
use wayland_server::protocol::{wl_surface::WlSurface, wl_buffer::WlBuffer};
use wayland_server::Resource;

pub struct State {
    pub surfaces: HashMap<u32, SurfaceData>,
}

#[derive(Default)]
pub struct SurfaceData {
    pub buffer: Option<WlBuffer>,
    pub pending_buffer: Option<WlBuffer>,
    pub size: (i32, i32),
}

impl State {
    pub fn new() -> Self {
        Self {
            surfaces: HashMap::new(),
        }
    }
    
    pub fn get_surface_data(&mut self, surface: &WlSurface) -> &mut SurfaceData {
        let id = surface.id().protocol_id();
        self.surfaces.entry(id).or_insert_with(SurfaceData::default)
    }
}

