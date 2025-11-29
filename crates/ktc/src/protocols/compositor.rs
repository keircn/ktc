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
                    window.pending_buffer = buffer;
                    window.pending_buffer_set = true;
                } else if let Some(ls) = state.get_layer_surface_by_wl_surface(resource) {
                    ls.pending_buffer = buffer;
                    ls.pending_buffer_set = true;
                }
            }
            wl_surface::Request::Commit => {
                let surface_id = resource.id();
                
                if let Some(window) = state.get_window_by_surface(resource) {
                    if window.pending_buffer_set {
                        window.buffer = window.pending_buffer.take();
                        window.pending_buffer_set = false;
                    }
                    window.mapped = window.buffer.is_some();
                    state.mark_surface_damage(surface_id.clone());
                } else if state.layer_surfaces.iter().any(|ls| ls.wl_surface.id() == surface_id) {
                    let (needs_initial_configure, needs_map) = {
                        let ls = state.layer_surfaces.iter_mut()
                            .find(|ls| ls.wl_surface.id() == surface_id);
                        if let Some(ls) = ls {
                            let needs_initial = !ls.configured;
                            
                            if ls.pending_buffer_set {
                                ls.buffer = ls.pending_buffer.take();
                                ls.pending_buffer_set = false;
                            }
                            let was_mapped = ls.mapped;
                            ls.mapped = ls.buffer.is_some();
                            ls.needs_redraw = true;
                            (needs_initial, !was_mapped && ls.mapped)
                        } else {
                            (false, false)
                        }
                    };
                    
                    if needs_initial_configure {
                        state.configure_layer_surface(surface_id.clone());
                    }
                    
                    if needs_map {
                        state.damage_tracker.mark_full_damage();
                    }
                    state.mark_layer_surface_damage(surface_id);
                }
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
                } else if let Some(ls) = state.get_layer_surface_by_wl_surface(resource) {
                    ls.needs_redraw = true;
                    let g = ls.geometry;
                    let rect = crate::state::Rectangle {
                        x: g.x + x,
                        y: g.y + y,
                        width,
                        height,
                    };
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
                } else if let Some(ls) = state.get_layer_surface_by_wl_surface(resource) {
                    ls.needs_redraw = true;
                    let g = ls.geometry;
                    let rect = crate::state::Rectangle {
                        x: g.x + x,
                        y: g.y + y,
                        width,
                        height,
                    };
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
                } else if state.layer_surfaces.iter().any(|ls| ls.wl_surface.id() == surface_id) {
                    log::info!("[surface] Found layer surface for surface {:?}, removing", surface_id);
                    state.remove_layer_surface_by_surface(resource);
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
        } else if state.layer_surfaces.iter().any(|ls| ls.wl_surface.id() == surface_id) {
            log::info!("[surface] Found layer surface for destroyed surface {:?}, removing", surface_id);
            state.remove_layer_surface_by_surface(resource);
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
