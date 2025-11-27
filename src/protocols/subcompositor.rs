use wayland_server::{GlobalDispatch, Dispatch, Resource};
use wayland_server::protocol::{
    wl_subcompositor::{self, WlSubcompositor},
    wl_subsurface::{self, WlSubsurface},
    wl_surface::WlSurface,
};
use crate::state::State;

impl GlobalDispatch<WlSubcompositor, ()> for State {
    fn bind(
        _state: &mut Self,
        _handle: &wayland_server::DisplayHandle,
        _client: &wayland_server::Client,
        resource: wayland_server::New<WlSubcompositor>,
        _global_data: &(),
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        log::info!("[subcompositor] Bound");
        data_init.init(resource, ());
    }
}

impl Dispatch<WlSubcompositor, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &WlSubcompositor,
        request: wl_subcompositor::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            wl_subcompositor::Request::GetSubsurface { id, surface, parent } => {
                log::info!("[subcompositor] GetSubsurface: surface={:?}, parent={:?}", 
                    surface.id(), parent.id());
                data_init.init(id, SubsurfaceData {
                    surface: surface.clone(),
                    parent: parent.clone(),
                });
            }
            wl_subcompositor::Request::Destroy => {
                log::info!("[subcompositor] Destroy");
            }
            _ => {}
        }
    }
}

pub struct SubsurfaceData {
    pub surface: WlSurface,
    pub parent: WlSurface,
}

impl Dispatch<WlSubsurface, SubsurfaceData> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &WlSubsurface,
        request: wl_subsurface::Request,
        _data: &SubsurfaceData,
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            wl_subsurface::Request::SetPosition { x, y } => {
                log::debug!("[subsurface] SetPosition: x={}, y={}", x, y);
            }
            wl_subsurface::Request::PlaceAbove { sibling } => {
                log::debug!("[subsurface] PlaceAbove: {:?}", sibling.id());
            }
            wl_subsurface::Request::PlaceBelow { sibling } => {
                log::debug!("[subsurface] PlaceBelow: {:?}", sibling.id());
            }
            wl_subsurface::Request::SetSync => {
                log::debug!("[subsurface] SetSync");
            }
            wl_subsurface::Request::SetDesync => {
                log::debug!("[subsurface] SetDesync");
            }
            wl_subsurface::Request::Destroy => {
                log::debug!("[subsurface] Destroy");
            }
            _ => {}
        }
    }
}
