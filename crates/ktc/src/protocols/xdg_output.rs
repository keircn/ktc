use wayland_server::{GlobalDispatch, Dispatch, Resource};
use wayland_server::protocol::wl_output::WlOutput;
use wayland_protocols::xdg::xdg_output::zv1::server::{
    zxdg_output_manager_v1::{self, ZxdgOutputManagerV1},
    zxdg_output_v1::{self, ZxdgOutputV1},
};
use crate::state::State;

impl GlobalDispatch<ZxdgOutputManagerV1, ()> for State {
    fn bind(
        _state: &mut Self,
        _handle: &wayland_server::DisplayHandle,
        _client: &wayland_server::Client,
        resource: wayland_server::New<ZxdgOutputManagerV1>,
        _global_data: &(),
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &ZxdgOutputManagerV1,
        request: zxdg_output_manager_v1::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            zxdg_output_manager_v1::Request::GetXdgOutput { id, output } => {
                let xdg_output = data_init.init(id, output.clone());
                state.send_xdg_output_info(&xdg_output, &output);
            }
            zxdg_output_manager_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputV1, WlOutput> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &ZxdgOutputV1,
        request: zxdg_output_v1::Request,
        _data: &WlOutput,
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        if let zxdg_output_v1::Request::Destroy = request {}
    }
}

impl State {
    pub fn send_xdg_output_info(&self, xdg_output: &ZxdgOutputV1, wl_output: &WlOutput) {
        if let Some(output) = self.outputs.first() {
            xdg_output.logical_position(output.x, output.y);

            let (logical_width, logical_height) = output.scaled_size();
            xdg_output.logical_size(logical_width, logical_height);

            if xdg_output.version() >= 2 {
                xdg_output.name(output.name.clone());
                
                let description = format!("{} {}", output.make, output.model);
                xdg_output.description(description);
            }

            if xdg_output.version() < 3 {
                xdg_output.done();
            } else {
                wl_output.done();
            }
        }
    }
}
