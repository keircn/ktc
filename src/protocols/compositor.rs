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
                data_init.init(id, ());
            }
            wl_compositor::Request::CreateRegion { id } => {
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
                if let Some(window) = state.get_window_by_surface(resource) {
                    if buffer.is_none() {
                        window.pending_buffer = None;
                        window.mapped = false;
                    } else {
                        window.pending_buffer = buffer;
                    }
                }
            }
            wl_surface::Request::Commit => {
                let surface_id = resource.id();
                if let Some(window) = state.get_window_by_surface(resource) {
                    let mut buffer_changed = false;
                    if let Some(buffer) = window.pending_buffer.take() {
                        let new_buffer_id = buffer.id().protocol_id();
                        buffer_changed = window.last_buffer_id != new_buffer_id;
                        window.last_buffer_id = new_buffer_id;
                        window.buffer = Some(buffer);
                    }
                    let was_mapped = window.mapped;
                    window.mapped = window.buffer.is_some();
                    
                    if buffer_changed || !was_mapped {
                        window.needs_redraw = true;
                    }
                }
                state.mark_surface_damage(surface_id);
            }
            wl_surface::Request::Frame { callback } => {
                let cb = data_init.init(callback, ());
                state.frame_callbacks.push(cb);
            }
            wl_surface::Request::Damage { x, y, width, height } => {
                let title_bar_height = state.title_bar_height();
                let damage_info = state.get_window_by_surface(resource).map(|window| {
                    window.needs_redraw = true;
                    let g = window.geometry;
                    crate::state::Rectangle {
                        x: g.x + x,
                        y: g.y + title_bar_height + y,
                        width,
                        height,
                    }
                });
                if let Some(rect) = damage_info {
                    state.damage_tracker.add_damage(rect);
                }
            }
            wl_surface::Request::DamageBuffer { x, y, width, height } => {
                let title_bar_height = state.title_bar_height();
                let damage_info = state.get_window_by_surface(resource).map(|window| {
                    window.needs_redraw = true;
                    let g = window.geometry;
                    crate::state::Rectangle {
                        x: g.x + x,
                        y: g.y + title_bar_height + y,
                        width,
                        height,
                    }
                });
                if let Some(rect) = damage_info {
                    state.damage_tracker.add_damage(rect);
                }
            }
            wl_surface::Request::Destroy => {
                let surface_id = resource.id();
                log::info!("[surface] Destroy request for surface {:?}", surface_id);
                if let Some(pos) = state.windows.iter().position(|w| w.wl_surface.id() == surface_id) {
                    let window_id = state.windows[pos].id;
                    log::info!("[surface] Found window {} for surface, removing", window_id);
                    state.remove_window(window_id);
                    state.relayout_windows();
                } else {
                    log::debug!("[surface] No window found for surface {:?}", surface_id);
                }
            }
            _ => {}
        }
    }
    
    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &WlSurface,
        _data: &(),
    ) {
        let surface_id = resource.id();
        log::info!("[surface] Surface {:?} destroyed (client disconnected or resource dropped)", surface_id);
        if let Some(pos) = state.windows.iter().position(|w| w.wl_surface.id() == surface_id) {
            let window_id = state.windows[pos].id;
            log::info!("[surface] Found window {} for destroyed surface, removing", window_id);
            state.remove_window(window_id);
            state.relayout_windows();
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
