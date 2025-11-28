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
            }
            wl_seat::Request::GetKeyboard { id } => {
                let keyboard = data_init.init(id, ());
                
                if let Some(ref keymap_data) = state.keymap_data {
                    keyboard.keymap(KeymapFormat::XkbV1, keymap_data.fd.as_fd(), keymap_data.size);
                }
                
                if keyboard.version() >= 4 {
                    keyboard.repeat_info(25, 600);
                }
                
                // Send keyboard.enter if there's a focused window belonging to this client
                let enter_info = state.focused_window.and_then(|focused_id| {
                    state.windows.iter()
                        .find(|w| w.id == focused_id)
                        .filter(|w| w.wl_surface.client() == keyboard.client())
                        .map(|w| (focused_id, w.wl_surface.clone()))
                });
                
                if let Some((focused_id, surface)) = enter_info {
                    let serial = state.next_keyboard_serial();
                    keyboard.enter(serial, &surface, vec![]);
                    state.keyboard_to_window.insert(keyboard.id(), focused_id);
                    log::info!("[seat] Sent keyboard.enter to newly created keyboard for focused window {}", focused_id);
                }
                
                state.keyboards.push(keyboard);
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
    
    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &WlPointer,
        _data: &(),
    ) {
        let pointer_id = resource.id();
        if let Some(pos) = state.pointers.iter().position(|p| p.id() == pointer_id) {
            state.pointers.swap_remove(pos);
            log::info!("[seat] Pointer {:?} destroyed, {} pointers remaining", pointer_id, state.pointers.len());
        }
    }
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
    
    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &WlKeyboard,
        _data: &(),
    ) {
        let keyboard_id = resource.id();
        state.keyboard_to_window.remove(&keyboard_id);
        if let Some(pos) = state.keyboards.iter().position(|k| k.id() == keyboard_id) {
            state.keyboards.swap_remove(pos);
            log::info!("[seat] Keyboard {:?} destroyed, {} keyboards remaining", keyboard_id, state.keyboards.len());
        }
    }
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
