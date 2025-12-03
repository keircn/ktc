use crate::state::State;
use wayland_server::protocol::{
    wl_subcompositor::{self, WlSubcompositor},
    wl_subsurface::{self, WlSubsurface},
    wl_surface::WlSurface,
};
use wayland_server::{Dispatch, GlobalDispatch, Resource};

impl GlobalDispatch<WlSubcompositor, ()> for State {
    fn bind(
        _state: &mut Self,
        _handle: &wayland_server::DisplayHandle,
        _client: &wayland_server::Client,
        resource: wayland_server::New<WlSubcompositor>,
        _global_data: &(),
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WlSubcompositor, ()> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &WlSubcompositor,
        request: wl_subcompositor::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        if let wl_subcompositor::Request::GetSubsurface {
            id,
            surface,
            parent,
        } = request
        {
            let surface_id = surface.id();
            let parent_id = parent.id();
            log::debug!(
                "[subcompositor] Creating subsurface: {:?} with parent {:?}",
                surface_id,
                parent_id
            );
            state.subsurfaces.insert(surface_id, parent_id);
            data_init.init(
                id,
                SubsurfaceData {
                    surface: surface.clone(),
                    parent: parent.clone(),
                },
            );
        }
    }
}

pub struct SubsurfaceData {
    #[allow(dead_code)]
    surface: WlSurface,
    #[allow(dead_code)]
    parent: WlSurface,
}

impl Dispatch<WlSubsurface, SubsurfaceData> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &WlSubsurface,
        _request: wl_subsurface::Request,
        _data: &SubsurfaceData,
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
    }
}
