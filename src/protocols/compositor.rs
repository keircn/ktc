use wayland_server::{GlobalDispatch, Dispatch, Resource};
use wayland_server::protocol::{
    wl_compositor::{self, WlCompositor},
    wl_surface::{self, WlSurface},
    wl_callback::WlCallback,
    wl_region::{self, WlRegion},
};
use crate::state::State;

impl GlobalDispatch<WlCompositor, ()> for State {
    fn bind(
        _state: &mut Self,
        _handle: &wayland_server::DisplayHandle,
        _client: &wayland_server::Client,
        resource: wayland_server::New<WlCompositor>,
        _global_data: &(),
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WlCompositor, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &WlCompositor,
        request: wl_compositor::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            wl_compositor::Request::CreateSurface { id } => {
                log::info!("[compositor] CreateSurface");
                let surface = data_init.init(id, ());
                let surface_data = _state.get_surface_data(&surface);
                surface_data.wl_surface = Some(surface);
            }
            wl_compositor::Request::CreateRegion { id } => {
                log::info!("[compositor] CreateRegion");
                data_init.init(id, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<WlSurface, ()> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        resource: &WlSurface,
        request: wl_surface::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            wl_surface::Request::Attach { buffer, .. } => {
                log::info!("[surface] Attach buffer: {:?}", buffer.as_ref().map(|b| b.id()));
                let surface_data = state.get_surface_data(resource);
                surface_data.pending_buffer = buffer;
            }
            wl_surface::Request::Commit => {
                log::info!("[surface] Commit");
                let surface_data = state.get_surface_data(resource);
                if let Some(buffer) = surface_data.pending_buffer.take() {
                    log::info!("[surface] Committing buffer: {:?}", buffer.id());
                    surface_data.buffer = Some(buffer);
                }
            }
            wl_surface::Request::Frame { callback } => {
                log::info!("[surface] Frame callback requested");
                let cb = data_init.init(callback, ());
                state.frame_callbacks.push(cb);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCallback, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &WlCallback,
        _request: wayland_server::protocol::wl_callback::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {}
}

impl Dispatch<WlRegion, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &WlRegion,
        _request: wl_region::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {}
}
