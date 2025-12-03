use crate::state::State;
use wayland_protocols::xdg::shell::server::{
    xdg_popup::{self, XdgPopup},
    xdg_positioner::{self, XdgPositioner},
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::{self, XdgWmBase},
};
use wayland_server::{Dispatch, GlobalDispatch, Resource};

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
                data_init.init(id, ());
            }
            xdg_wm_base::Request::GetXdgSurface { id, surface } => {
                let xdg_surface = data_init.init(id, ());
                let xdg_id = xdg_surface.id().protocol_id();
                state
                    .pending_xdg_surfaces
                    .insert(xdg_id, (xdg_surface, surface));
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
    ) {
    }
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
                let toplevel = data_init.init(id, ());

                let xdg_id = resource.id().protocol_id();
                if let Some((xdg_surface, wl_surface)) = state.pending_xdg_surfaces.remove(&xdg_id)
                {
                    let window_id = state.add_window_without_relayout(
                        xdg_surface,
                        toplevel.clone(),
                        wl_surface,
                    );
                    log::info!("Window {} created", window_id);

                    let tiling_states = state.get_toplevel_states(window_id);
                    let (geometry_width, geometry_height) =
                        if let Some(window) = state.get_window_mut(window_id) {
                            (window.geometry.width, window.geometry.height)
                        } else {
                            state.screen_size()
                        };

                    toplevel.configure(geometry_width, geometry_height, tiling_states);
                    let serial = state.next_keyboard_serial();
                    resource.configure(serial);
                    state.set_focus_without_relayout(window_id);
                    state.needs_relayout = true;
                }
            }
            xdg_surface::Request::GetPopup { id, .. } => {
                data_init.init(id, ());
            }
            xdg_surface::Request::AckConfigure { .. } => {}
            xdg_surface::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<XdgToplevel, ()> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        resource: &XdgToplevel,
        request: xdg_toplevel::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            xdg_toplevel::Request::SetTitle { title } => {
                if let Some(window) = state
                    .windows
                    .iter_mut()
                    .find(|w| w.xdg_toplevel.id() == resource.id())
                {
                    let old_title = window.title.clone();
                    window.title = title.clone();
                    let window_id = window.id;
                    let is_focused = state.focused_window == Some(window_id);

                    if is_focused && old_title != title {
                        state.pending_title_change = Some(title);
                    }
                }
            }
            xdg_toplevel::Request::SetAppId { .. } => {}
            xdg_toplevel::Request::SetParent { .. } => {}
            xdg_toplevel::Request::ShowWindowMenu { .. } => {}
            xdg_toplevel::Request::Move { .. } => {}
            xdg_toplevel::Request::Resize { .. } => {}
            xdg_toplevel::Request::SetMaxSize { .. } => {}
            xdg_toplevel::Request::SetMinSize { .. } => {}
            xdg_toplevel::Request::SetMaximized => {}
            xdg_toplevel::Request::UnsetMaximized => {}
            xdg_toplevel::Request::SetFullscreen { .. } => {}
            xdg_toplevel::Request::UnsetFullscreen => {}
            xdg_toplevel::Request::SetMinimized => {}
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
    ) {
    }
}
