use std::sync::Mutex;
use std::os::fd::OwnedFd;
use wayland_server::{Dispatch, GlobalDispatch, Resource};
use wayland_protocols::wp::linux_dmabuf::zv1::server::{
    zwp_linux_dmabuf_v1::{self, ZwpLinuxDmabufV1},
    zwp_linux_buffer_params_v1::{self, ZwpLinuxBufferParamsV1},
};
use wayland_server::protocol::wl_buffer::WlBuffer;
use crate::state::State;

pub struct DmaBufGlobal;

pub struct DmaBufParamsData {
    inner: Mutex<DmaBufParamsInner>,
}

impl Default for DmaBufParamsData {
    fn default() -> Self {
        Self {
            inner: Mutex::new(DmaBufParamsInner::default()),
        }
    }
}

#[derive(Default)]
struct DmaBufParamsInner {
    width: i32,
    height: i32,
    format: u32,
    planes: Vec<DmaBufPlane>,
}

pub struct DmaBufPlane {
    #[allow(dead_code)]
    fd: OwnedFd,
    #[allow(dead_code)]
    plane_idx: u32,
    #[allow(dead_code)]
    offset: u32,
    #[allow(dead_code)]
    stride: u32,
    #[allow(dead_code)]
    modifier_hi: u32,
    #[allow(dead_code)]
    modifier_lo: u32,
}

pub struct DmaBufBufferData {
    #[allow(dead_code)]
    width: i32,
    #[allow(dead_code)]
    height: i32,
    #[allow(dead_code)]
    format: u32,
    #[allow(dead_code)]
    planes: Vec<DmaBufPlane>,
}

impl GlobalDispatch<ZwpLinuxDmabufV1, DmaBufGlobal> for State {
    fn bind(
        state: &mut Self,
        _handle: &wayland_server::DisplayHandle,
        _client: &wayland_server::Client,
        resource: wayland_server::New<ZwpLinuxDmabufV1>,
        _global_data: &DmaBufGlobal,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        let dmabuf = data_init.init(resource, ());
        
        if let Some(ref renderer) = state.gpu_renderer {
            for fmt in &renderer.supported_formats {
                if dmabuf.version() >= 3 {
                    dmabuf.modifier(
                        fmt.format,
                        (fmt.modifier >> 32) as u32,
                        (fmt.modifier & 0xFFFFFFFF) as u32,
                    );
                } else {
                    dmabuf.format(fmt.format);
                }
            }
        } else {
            dmabuf.format(drm_fourcc::DrmFourcc::Argb8888 as u32);
            dmabuf.format(drm_fourcc::DrmFourcc::Xrgb8888 as u32);
        }
    }
}

impl Dispatch<ZwpLinuxDmabufV1, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &ZwpLinuxDmabufV1,
        request: zwp_linux_dmabuf_v1::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            zwp_linux_dmabuf_v1::Request::CreateParams { params_id } => {
                data_init.init(params_id, DmaBufParamsData::default());
            }
            zwp_linux_dmabuf_v1::Request::Destroy => {}
            zwp_linux_dmabuf_v1::Request::GetDefaultFeedback { .. } => {
                log::warn!("[dmabuf] get_default_feedback not implemented");
            }
            zwp_linux_dmabuf_v1::Request::GetSurfaceFeedback { .. } => {
                log::warn!("[dmabuf] get_surface_feedback not implemented");
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpLinuxBufferParamsV1, DmaBufParamsData> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        resource: &ZwpLinuxBufferParamsV1,
        request: zwp_linux_buffer_params_v1::Request,
        data: &DmaBufParamsData,
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            zwp_linux_buffer_params_v1::Request::Add { fd, plane_idx, offset, stride, modifier_hi, modifier_lo } => {
                let mut inner = data.inner.lock().unwrap();
                
                inner.planes.push(DmaBufPlane {
                    fd,
                    plane_idx,
                    offset,
                    stride,
                    modifier_hi,
                    modifier_lo,
                });
                
                log::debug!(
                    "[dmabuf] Added plane {}: offset={}, stride={}, modifier=0x{:08x}{:08x}",
                    plane_idx, offset, stride, modifier_hi, modifier_lo
                );
            }
            zwp_linux_buffer_params_v1::Request::Create { width, height, format, .. } => {
                log::info!("[dmabuf] Create buffer: {}x{} format=0x{:08x}", width, height, format);
                
                let mut inner = data.inner.lock().unwrap();
                inner.width = width;
                inner.height = height;
                inner.format = format;
                
                if state.gpu_renderer.is_some() && !inner.planes.is_empty() {
                    let buffer_data = DmaBufBufferData {
                        width,
                        height,
                        format,
                        planes: std::mem::take(&mut inner.planes),
                    };
                    
                    resource.created(&create_dmabuf_buffer(state, data_init, buffer_data));
                } else {
                    resource.failed();
                }
            }
            zwp_linux_buffer_params_v1::Request::CreateImmed { buffer_id, width, height, format, .. } => {
                log::info!("[dmabuf] CreateImmed buffer: {}x{} format=0x{:08x}", width, height, format);
                
                let mut inner = data.inner.lock().unwrap();
                inner.width = width;
                inner.height = height;
                inner.format = format;
                
                let buffer_data = DmaBufBufferData {
                    width,
                    height,
                    format,
                    planes: std::mem::take(&mut inner.planes),
                };
                
                data_init.init(buffer_id, buffer_data);
            }
            zwp_linux_buffer_params_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

fn create_dmabuf_buffer(
    _state: &mut State,
    _data_init: &mut wayland_server::DataInit<'_, State>,
    _buffer_data: DmaBufBufferData,
) -> WlBuffer {
    // TODO: create a proper wl_buffer
    todo!("create_dmabuf_buffer for async Create path")
}

impl Dispatch<WlBuffer, DmaBufBufferData> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        resource: &WlBuffer,
        request: wayland_server::protocol::wl_buffer::Request,
        data: &DmaBufBufferData,
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        if let wayland_server::protocol::wl_buffer::Request::Destroy = request {
            log::debug!("[dmabuf] Buffer destroyed: {}x{}", data.width, data.height);
            if let Some(ref mut renderer) = state.gpu_renderer {
                renderer.remove_texture(resource.id().protocol_id() as u64);
            }
        }
    }
}
