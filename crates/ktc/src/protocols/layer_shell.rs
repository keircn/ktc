use wayland_server::{Dispatch, GlobalDispatch, Resource, WEnum};
use wayland_protocols_wlr::layer_shell::v1::server::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1, Layer as WlrLayer},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1, KeyboardInteractivity},
};
use wayland_server::protocol::wl_surface::WlSurface;
use crate::state::{State, Rectangle, LayerSurface, Layer};

pub struct LayerShellGlobal;

fn convert_layer(layer: WEnum<WlrLayer>) -> Layer {
    match layer {
        WEnum::Value(WlrLayer::Background) => Layer::Background,
        WEnum::Value(WlrLayer::Bottom) => Layer::Bottom,
        WEnum::Value(WlrLayer::Top) => Layer::Top,
        WEnum::Value(WlrLayer::Overlay) => Layer::Overlay,
        _ => Layer::Top,
    }
}

fn convert_anchor(anchor: WEnum<Anchor>) -> Anchor {
    match anchor {
        WEnum::Value(a) => a,
        _ => Anchor::empty(),
    }
}

fn convert_keyboard_interactivity(ki: WEnum<KeyboardInteractivity>) -> KeyboardInteractivity {
    match ki {
        WEnum::Value(k) => k,
        _ => KeyboardInteractivity::None,
    }
}

impl GlobalDispatch<ZwlrLayerShellV1, LayerShellGlobal> for State {
    fn bind(
        _state: &mut Self,
        _handle: &wayland_server::DisplayHandle,
        _client: &wayland_server::Client,
        resource: wayland_server::New<ZwlrLayerShellV1>,
        _global_data: &LayerShellGlobal,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &ZwlrLayerShellV1,
        request: zwlr_layer_shell_v1::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            zwlr_layer_shell_v1::Request::GetLayerSurface {
                id,
                surface,
                output: _,
                layer,
                namespace,
            } => {
                let layer_value = convert_layer(layer);

                let layer_surface_data = LayerSurfaceData {
                    surface: surface.clone(),
                    namespace: namespace.clone(),
                    layer: layer_value,
                    anchor: Anchor::empty(),
                    exclusive_zone: 0,
                    margin: (0, 0, 0, 0),
                    keyboard_interactivity: KeyboardInteractivity::None,
                    desired_size: (0, 0),
                    configured: false,
                };

                let layer_surface = data_init.init(id, layer_surface_data);

                let id = state.next_layer_surface_id;
                state.next_layer_surface_id += 1;

                state.layer_surfaces.push(LayerSurface {
                    id,
                    wl_surface: surface,
                    layer_surface,
                    layer: layer_value,
                    namespace,
                    anchor: Anchor::empty(),
                    exclusive_zone: 0,
                    margin: (0, 0, 0, 0),
                    keyboard_interactivity: KeyboardInteractivity::None,
                    geometry: Rectangle::default(),
                    desired_width: 0,
                    desired_height: 0,
                    configured: false,
                    mapped: false,
                    buffer: None,
                    pending_buffer: None,
                    pending_buffer_set: false,
                    buffer_released: true,
                    needs_redraw: true,
                    pixel_cache: Vec::new(),
                    cache_width: 0,
                    cache_height: 0,
                    cache_stride: 0,
                });

                log::debug!("[layer_shell] Created layer surface {}", id);
            }
            zwlr_layer_shell_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

#[derive(Clone)]
pub struct LayerSurfaceData {
    pub surface: WlSurface,
    pub namespace: String,
    pub layer: Layer,
    pub anchor: Anchor,
    pub exclusive_zone: i32,
    pub margin: (i32, i32, i32, i32),
    pub keyboard_interactivity: KeyboardInteractivity,
    pub desired_size: (u32, u32),
    pub configured: bool,
}

impl Dispatch<ZwlrLayerSurfaceV1, LayerSurfaceData> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &ZwlrLayerSurfaceV1,
        request: zwlr_layer_surface_v1::Request,
        data: &LayerSurfaceData,
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        let surface_id = data.surface.id();

        match request {
            zwlr_layer_surface_v1::Request::SetSize { width, height } => {
                if let Some(ls) = state.layer_surfaces.iter_mut().find(|ls| ls.wl_surface.id() == surface_id) {
                    ls.desired_width = width;
                    ls.desired_height = height;
                }
            }
            zwlr_layer_surface_v1::Request::SetAnchor { anchor } => {
                if let Some(ls) = state.layer_surfaces.iter_mut().find(|ls| ls.wl_surface.id() == surface_id) {
                    ls.anchor = convert_anchor(anchor);
                }
            }
            zwlr_layer_surface_v1::Request::SetExclusiveZone { zone } => {
                if let Some(ls) = state.layer_surfaces.iter_mut().find(|ls| ls.wl_surface.id() == surface_id) {
                    ls.exclusive_zone = zone;
                }
            }
            zwlr_layer_surface_v1::Request::SetMargin { top, right, bottom, left } => {
                if let Some(ls) = state.layer_surfaces.iter_mut().find(|ls| ls.wl_surface.id() == surface_id) {
                    ls.margin = (top, right, bottom, left);
                }
            }
            zwlr_layer_surface_v1::Request::SetKeyboardInteractivity { keyboard_interactivity } => {
                if let Some(ls) = state.layer_surfaces.iter_mut().find(|ls| ls.wl_surface.id() == surface_id) {
                    ls.keyboard_interactivity = convert_keyboard_interactivity(keyboard_interactivity);
                }
            }
            zwlr_layer_surface_v1::Request::GetPopup { popup: _ } => {}
            zwlr_layer_surface_v1::Request::AckConfigure { serial: _ } => {}
            zwlr_layer_surface_v1::Request::Destroy => {
                state.remove_layer_surface_by_surface(&data.surface);
            }
            zwlr_layer_surface_v1::Request::SetLayer { layer } => {
                let layer_value = convert_layer(layer);
                if let Some(ls) = state.layer_surfaces.iter_mut().find(|ls| ls.wl_surface.id() == surface_id) {
                    ls.layer = layer_value;
                }
            }
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        _resource: &ZwlrLayerSurfaceV1,
        data: &LayerSurfaceData,
    ) {
        state.remove_layer_surface_by_surface(&data.surface);
    }
}

impl State {
    pub fn configure_layer_surface(&mut self, surface_id: wayland_server::backend::ObjectId) {
        let (screen_width, screen_height) = self.screen_size();

        let (anchor, margin, desired_width, desired_height, layer_surface) = {
            let ls = match self.layer_surfaces.iter_mut().find(|ls| ls.wl_surface.id() == surface_id) {
                Some(ls) => ls,
                None => return,
            };
            (ls.anchor, ls.margin, ls.desired_width, ls.desired_height, ls.layer_surface.clone())
        };

        let anchored_left = anchor.contains(Anchor::Left);
        let anchored_right = anchor.contains(Anchor::Right);
        let anchored_top = anchor.contains(Anchor::Top);
        let anchored_bottom = anchor.contains(Anchor::Bottom);

        // i stole this from hyprland's implementation :3
        let mut width = desired_width as i32;
        let mut height = desired_height as i32;
        
        let x = if width == 0 {
            margin.3
        } else if anchored_left && anchored_right {
            (screen_width - width) / 2
        } else if anchored_left {
            margin.3
        } else if anchored_right {
            screen_width - width - margin.1
        } else {
            (screen_width - width) / 2
        };
        
        let y = if height == 0 {
            margin.0
        } else if anchored_top && anchored_bottom {
            (screen_height - height) / 2
        } else if anchored_top {
            margin.0
        } else if anchored_bottom {
            screen_height - height - margin.2
        } else {
            (screen_height - height) / 2
        };
        
        if width == 0 {
            width = screen_width - margin.3 - margin.1;
        }
        
        if height == 0 {
            height = screen_height - margin.0 - margin.2;
        }

        if let Some(ls) = self.layer_surfaces.iter_mut().find(|ls| ls.wl_surface.id() == surface_id) {
            ls.geometry = Rectangle { x, y, width, height };
            ls.configured = true;
        }

        let serial = self.next_keyboard_serial();
        layer_surface.configure(serial, width as u32, height as u32);

        log::debug!(
            "[layer_shell] Configured surface: {}x{} at ({}, {})",
            width, height, x, y
        );
    }

    pub fn remove_layer_surface_by_surface(&mut self, surface: &WlSurface) {
        let surface_id = surface.id();
        if let Some(pos) = self.layer_surfaces.iter().position(|ls| ls.wl_surface.id() == surface_id) {
            let ls = &self.layer_surfaces[pos];
            log::debug!("[layer_shell] Removing layer surface {} (namespace: {})", ls.id, ls.namespace);
            ls.layer_surface.closed();
            self.damage_tracker.add_damage(ls.geometry);
            self.layer_surfaces.swap_remove(pos);
            self.damage_tracker.mark_full_damage();
        }
    }

    pub fn get_layer_surface_by_wl_surface(&mut self, surface: &WlSurface) -> Option<&mut LayerSurface> {
        let surface_id = surface.id();
        self.layer_surfaces.iter_mut().find(|ls| ls.wl_surface.id() == surface_id)
    }

    pub fn map_layer_surface(&mut self, surface_id: wayland_server::backend::ObjectId) {
        if let Some(ls) = self.layer_surfaces.iter_mut().find(|ls| ls.wl_surface.id() == surface_id) {
            if !ls.mapped {
                ls.mapped = true;
                self.damage_tracker.mark_full_damage();
                log::debug!("[layer_shell] Mapped layer surface {}", ls.id);
            }
        }
    }

    pub fn layer_surfaces_by_layer(&self, layer: Layer) -> impl Iterator<Item = &LayerSurface> {
        self.layer_surfaces.iter().filter(move |ls| ls.layer == layer && ls.mapped)
    }
}
