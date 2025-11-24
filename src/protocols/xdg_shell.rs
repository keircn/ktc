use wayland_server::{GlobalDispatch, Dispatch};
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
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &XdgWmBase,
        request: xdg_wm_base::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            xdg_wm_base::Request::CreatePositioner { id } => {
                eprintln!("[xdg_wm_base] CreatePositioner");
                data_init.init(id, ());
            }
            xdg_wm_base::Request::GetXdgSurface { id, .. } => {
                eprintln!("[xdg_wm_base] GetXdgSurface");
                data_init.init(id, ());
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
        _state: &mut Self,
        _client: &wayland_server::Client,
        resource: &XdgSurface,
        request: xdg_surface::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            xdg_surface::Request::GetToplevel { id } => {
                eprintln!("[xdg_surface] GetToplevel");
                let toplevel = data_init.init(id, ());
                resource.configure(1);
                toplevel.configure(0, 0, vec![]);
            }
            xdg_surface::Request::GetPopup { id, .. } => {
                eprintln!("[xdg_surface] GetPopup");
                data_init.init(id, ());
            }
            xdg_surface::Request::AckConfigure { .. } => {
                eprintln!("[xdg_surface] AckConfigure");
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
        _request: xdg_toplevel::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {}
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
