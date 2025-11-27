use wayland_server::{GlobalDispatch, Dispatch, Resource};
use wayland_protocols::xdg::shell::server::{
    xdg_wm_base::{self, XdgWmBase},
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_popup::{self, XdgPopup},
    xdg_positioner::{self, XdgPositioner},
};
use crate::state::State;

impl GlobalDispatch<XdgWmBase, ()> for State {
    fn bind(
        _state: &mut Self,
        _handle: &wayland_server::DisplayHandle,
        _client: &wayland_server::Client,
        resource: wayland_server::New<XdgWmBase>,
        _global_data: &(),
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<XdgWmBase, ()> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &XdgWmBase,
        request: xdg_wm_base::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            xdg_wm_base::Request::CreatePositioner { id } => {
                log::info!("[xdg_wm_base] CreatePositioner");
                data_init.init(id, ());
            }
            xdg_wm_base::Request::GetXdgSurface { id, surface } => {
                log::info!("[xdg_wm_base] GetXdgSurface for surface {:?}", surface.id());
                let xdg_surface = data_init.init(id, ());
                let xdg_id = xdg_surface.id().protocol_id();
                state.pending_xdg_surfaces.insert(xdg_id, (xdg_surface, surface));
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgPositioner, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &XdgPositioner,
        _request: xdg_positioner::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {}
}

impl Dispatch<XdgSurface, ()> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        resource: &XdgSurface,
        request: xdg_surface::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            xdg_surface::Request::GetToplevel { id } => {
                log::info!("[xdg_surface] GetToplevel");
                let toplevel = data_init.init(id, ());
                
                let xdg_id = resource.id().protocol_id();
                if let Some((xdg_surface, wl_surface)) = state.pending_xdg_surfaces.remove(&xdg_id) {
                    let window_id = state.add_window_without_relayout(xdg_surface, toplevel.clone(), wl_surface);
                    log::info!("[xdg_surface] Created window id={}", window_id);
                    
                    let tiling_states = state.get_toplevel_states(window_id);
                    log::info!("[xdg_surface] Got tiling states for window {}", window_id);
                    
                    let (geometry_width, geometry_height) = if let Some(window) = state.get_window_mut(window_id) {
                        (window.geometry.width, window.geometry.height)
                    } else {
                        state.screen_size()
                    };
                    log::info!("[xdg_surface] Window {} geometry: {}x{}", window_id, geometry_width, geometry_height);
                    
                    log::info!("[xdg_surface] Sending toplevel.configure for window {}", window_id);
                    toplevel.configure(geometry_width, geometry_height, tiling_states);
                    let serial = state.next_keyboard_serial();
                    log::info!("[xdg_surface] Sending xdg_surface.configure (serial={}) for window {}", serial, window_id);
                    resource.configure(serial);
                    log::info!("[xdg_surface] Setting focus to window {}", window_id);
                    state.set_focus_without_relayout(window_id);
                    state.needs_relayout = true;
                    log::info!("[xdg_surface] Done processing GetToplevel for window {}", window_id);
                } else {
                    log::warn!("[xdg_surface] GetToplevel called but no pending XdgSurface found");
                }
            }
            xdg_surface::Request::GetPopup { id, .. } => {
                log::info!("[xdg_surface] GetPopup");
                data_init.init(id, ());
            }
            xdg_surface::Request::AckConfigure { serial } => {
                log::info!("[xdg_surface] AckConfigure: serial={}", serial);
            }
            xdg_surface::Request::Destroy => {
                log::info!("[xdg_surface] Destroy");
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgToplevel, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &XdgToplevel,
        request: xdg_toplevel::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            xdg_toplevel::Request::Destroy => {
                log::info!("[xdg_toplevel] Destroy");
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgPopup, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &XdgPopup,
        _request: xdg_popup::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {}
}
