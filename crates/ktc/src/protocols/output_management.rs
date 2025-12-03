use crate::state::{OutputId, OutputTransform, State};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Mutex,
};
use wayland_protocols_wlr::output_management::v1::server::{
    zwlr_output_configuration_head_v1::{self, ZwlrOutputConfigurationHeadV1},
    zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
    zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
    zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
    zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
};
use wayland_server::backend::ObjectId;
use wayland_server::protocol::wl_output::Transform;
use wayland_server::{Dispatch, DisplayHandle, GlobalDispatch, Resource};

static CONFIG_SERIAL: AtomicU32 = AtomicU32::new(1);

pub struct OutputManagerGlobal;

pub struct OutputManagerData {
    pub inner: Mutex<OutputManagerDataInner>,
}

pub struct OutputManagerDataInner {
    pub heads: HashMap<OutputId, ZwlrOutputHeadV1>,
    pub modes: HashMap<OutputId, ZwlrOutputModeV1>,
}

impl Default for OutputManagerData {
    fn default() -> Self {
        Self {
            inner: Mutex::new(OutputManagerDataInner {
                heads: HashMap::new(),
                modes: HashMap::new(),
            }),
        }
    }
}

pub struct OutputHeadData {
    pub output_id: OutputId,
}

#[allow(dead_code)]
pub struct OutputModeData {
    pub output_id: OutputId,
    pub width: i32,
    pub height: i32,
    pub refresh: i32,
}

pub struct OutputConfigurationData {
    pub serial: u32,
    pub used: bool,
}

#[allow(dead_code)]
pub struct ConfiguredHead {
    pub output_id: OutputId,
    pub enabled: bool,
    pub mode_width: Option<i32>,
    pub mode_height: Option<i32>,
    pub mode_refresh: Option<i32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub transform: Option<i32>,
    pub scale: Option<f64>,
}

#[allow(dead_code)]
pub struct OutputConfigurationHeadData {
    pub output_id: OutputId,
    pub config_id: ObjectId,
}

fn output_transform_to_wl(t: OutputTransform) -> Transform {
    match t {
        OutputTransform::Normal => Transform::Normal,
        OutputTransform::Rotate90 => Transform::_90,
        OutputTransform::Rotate180 => Transform::_180,
        OutputTransform::Rotate270 => Transform::_270,
        OutputTransform::Flipped => Transform::Flipped,
        OutputTransform::FlippedRotate90 => Transform::Flipped90,
        OutputTransform::FlippedRotate180 => Transform::Flipped180,
        OutputTransform::FlippedRotate270 => Transform::Flipped270,
    }
}

impl GlobalDispatch<ZwlrOutputManagerV1, OutputManagerGlobal> for State {
    fn bind(
        state: &mut Self,
        dhandle: &DisplayHandle,
        client: &wayland_server::Client,
        resource: wayland_server::New<ZwlrOutputManagerV1>,
        _global_data: &OutputManagerGlobal,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        let manager = data_init.init(resource, OutputManagerData::default());
        state.send_output_manager_state(&manager, dhandle, client);
    }
}

impl Dispatch<ZwlrOutputManagerV1, OutputManagerData> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        resource: &ZwlrOutputManagerV1,
        request: zwlr_output_manager_v1::Request,
        data: &OutputManagerData,
        _dhandle: &DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            zwlr_output_manager_v1::Request::CreateConfiguration { id, serial } => {
                let current_serial = CONFIG_SERIAL.load(Ordering::Relaxed);
                let config_data = OutputConfigurationData {
                    serial,
                    used: false,
                };
                let config = data_init.init(id, config_data);

                if serial != current_serial {
                    config.cancelled();
                }
            }
            zwlr_output_manager_v1::Request::Stop => {
                let inner = data.inner.lock().unwrap();
                for head in inner.heads.values() {
                    head.finished();
                }
                for mode in inner.modes.values() {
                    mode.finished();
                }
                resource.finished();
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrOutputHeadV1, OutputHeadData> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &ZwlrOutputHeadV1,
        request: zwlr_output_head_v1::Request,
        _data: &OutputHeadData,
        _dhandle: &DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        if let zwlr_output_head_v1::Request::Release = request {}
    }
}

impl Dispatch<ZwlrOutputModeV1, OutputModeData> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &ZwlrOutputModeV1,
        request: zwlr_output_mode_v1::Request,
        _data: &OutputModeData,
        _dhandle: &DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        if let zwlr_output_mode_v1::Request::Release = request {}
    }
}

impl Dispatch<ZwlrOutputConfigurationV1, OutputConfigurationData> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        resource: &ZwlrOutputConfigurationV1,
        request: zwlr_output_configuration_v1::Request,
        data: &OutputConfigurationData,
        _dhandle: &DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            zwlr_output_configuration_v1::Request::EnableHead { id, head } => {
                let head_data: &OutputHeadData = head.data().unwrap();
                let output_id = head_data.output_id;

                let config_head_data = OutputConfigurationHeadData {
                    output_id,
                    config_id: resource.id(),
                };
                let _config_head = data_init.init(id, config_head_data);
            }
            zwlr_output_configuration_v1::Request::DisableHead { head: _ } => {}
            zwlr_output_configuration_v1::Request::Apply => {
                if data.used {
                    return;
                }

                let current_serial = CONFIG_SERIAL.load(Ordering::Relaxed);
                if data.serial != current_serial {
                    resource.cancelled();
                    return;
                }

                resource.succeeded();

                CONFIG_SERIAL.fetch_add(1, Ordering::Relaxed);
                state.broadcast_output_manager_done();
            }
            zwlr_output_configuration_v1::Request::Test => {
                if data.used {
                    return;
                }

                let current_serial = CONFIG_SERIAL.load(Ordering::Relaxed);
                if data.serial != current_serial {
                    resource.cancelled();
                    return;
                }

                resource.succeeded();
            }
            zwlr_output_configuration_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<ZwlrOutputConfigurationHeadV1, OutputConfigurationHeadData> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &ZwlrOutputConfigurationHeadV1,
        request: zwlr_output_configuration_head_v1::Request,
        _data: &OutputConfigurationHeadData,
        _dhandle: &DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            zwlr_output_configuration_head_v1::Request::SetMode { mode: _ } => {}
            zwlr_output_configuration_head_v1::Request::SetCustomMode {
                width: _,
                height: _,
                refresh: _,
            } => {}
            zwlr_output_configuration_head_v1::Request::SetPosition { x: _, y: _ } => {}
            zwlr_output_configuration_head_v1::Request::SetTransform { transform: _ } => {}
            zwlr_output_configuration_head_v1::Request::SetScale { scale: _ } => {}
            zwlr_output_configuration_head_v1::Request::SetAdaptiveSync { state: _ } => {}
            _ => {}
        }
    }
}

impl State {
    fn send_output_manager_state(
        &self,
        manager: &ZwlrOutputManagerV1,
        dhandle: &DisplayHandle,
        client: &wayland_server::Client,
    ) {
        let manager_version = manager.version();
        let manager_data = manager.data::<OutputManagerData>().unwrap();

        for output in &self.outputs {
            let head_version = manager_version.min(4);
            let head: ZwlrOutputHeadV1 = client
                .create_resource::<ZwlrOutputHeadV1, _, Self>(
                    dhandle,
                    head_version,
                    OutputHeadData {
                        output_id: output.id,
                    },
                )
                .unwrap();

            manager.head(&head);

            head.name(output.name.clone());
            head.description(format!("{} {}", output.make, output.model));

            if output.physical_width > 0 && output.physical_height > 0 {
                head.physical_size(output.physical_width, output.physical_height);
            }

            let mode_version = head_version.min(3);
            let mode: ZwlrOutputModeV1 = client
                .create_resource::<ZwlrOutputModeV1, _, Self>(
                    dhandle,
                    mode_version,
                    OutputModeData {
                        output_id: output.id,
                        width: output.width,
                        height: output.height,
                        refresh: output.refresh,
                    },
                )
                .unwrap();

            head.mode(&mode);

            mode.size(output.width, output.height);
            mode.refresh(output.refresh);
            mode.preferred();

            head.enabled(1);
            head.current_mode(&mode);
            head.position(output.x, output.y);
            head.transform(output_transform_to_wl(output.transform));
            head.scale(output.scale as f64);

            if head.version() >= 2 {
                head.make(output.make.clone());
                head.model(output.model.clone());
            }

            if head.version() >= 4 {
                head.adaptive_sync(zwlr_output_head_v1::AdaptiveSyncState::Disabled);
            }

            let mut inner = manager_data.inner.lock().unwrap();
            inner.heads.insert(output.id, head);
            inner.modes.insert(output.id, mode);
        }

        let serial = CONFIG_SERIAL.load(Ordering::Relaxed);
        manager.done(serial);
    }

    pub fn broadcast_output_manager_done(&self) {
        let serial = CONFIG_SERIAL.load(Ordering::Relaxed);
        let _ = serial;
    }
}
