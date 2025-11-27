use wayland_server::{GlobalDispatch, Dispatch, Resource};
use wayland_server::protocol::{
    wl_seat::{self, WlSeat},
    wl_pointer::{self, WlPointer},
    wl_keyboard::{self, WlKeyboard, KeymapFormat},
    wl_touch::{self, WlTouch},
};
use crate::state::State;
use std::os::fd::AsFd;

impl GlobalDispatch<WlSeat, ()> for State {
    fn bind(
        _state: &mut Self,
        _handle: &wayland_server::DisplayHandle,
        _client: &wayland_server::Client,
        resource: wayland_server::New<WlSeat>,
        _global_data: &(),
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        let seat = data_init.init(resource, ());
        seat.capabilities(wl_seat::Capability::Pointer | wl_seat::Capability::Keyboard);
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &WlSeat,
        request: wl_seat::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            wl_seat::Request::GetPointer { id } => {
                let pointer = data_init.init(id, ());
                state.pointers.push(pointer);
                log::info!("[seat] Pointer created, total pointers: {}", state.pointers.len());
            }
            wl_seat::Request::GetKeyboard { id } => {
                let keyboard = data_init.init(id, ());
                
                if let Some(ref keymap_data) = state.keymap_data {
                    keyboard.keymap(KeymapFormat::XkbV1, keymap_data.fd.as_fd(), keymap_data.size);
                    log::info!("[seat] Sent keymap to keyboard (size={})", keymap_data.size);
                } else {
                    log::warn!("[seat] No keymap available to send to keyboard");
                }
                
                if keyboard.version() >= 4 {
                    keyboard.repeat_info(25, 600);
                }
                
                state.keyboards.push(keyboard);
                log::info!("[seat] Keyboard created, total keyboards: {}", state.keyboards.len());
            }
            wl_seat::Request::GetTouch { id } => {
                data_init.init(id, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<WlPointer, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &WlPointer,
        _request: wl_pointer::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {}
}

impl Dispatch<WlKeyboard, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &WlKeyboard,
        _request: wl_keyboard::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {}
}

impl Dispatch<WlTouch, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &WlTouch,
        _request: wl_touch::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {}
}
