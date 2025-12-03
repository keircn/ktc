use crate::state::State;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::Mutex;
use wayland_protocols::wp::linux_dmabuf::zv1::server::{
    zwp_linux_buffer_params_v1::{self, ZwpLinuxBufferParamsV1},
    zwp_linux_dmabuf_feedback_v1::{self, ZwpLinuxDmabufFeedbackV1},
    zwp_linux_dmabuf_v1::{self, ZwpLinuxDmabufV1},
};
use wayland_server::protocol::wl_buffer::WlBuffer;
use wayland_server::{Dispatch, GlobalDispatch, Resource};

pub struct DmaBufGlobal;

pub struct DmaBufFeedbackData {
    #[allow(dead_code)]
    pub for_surface: bool,
}

#[repr(C, packed)]
struct FormatModifierEntry {
    format: u32,
    _padding: u32,
    modifier: u64,
}

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
    pub fd: OwnedFd,
    #[allow(dead_code)]
    pub plane_idx: u32,
    pub offset: u32,
    pub stride: u32,
    pub modifier_hi: u32,
    pub modifier_lo: u32,
}

pub struct DmaBufBufferData {
    pub width: i32,
    pub height: i32,
    pub format: u32,
    pub planes: Vec<DmaBufPlane>,
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

        if dmabuf.version() >= 4 {
            return;
        }

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
        state: &mut Self,
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
            zwp_linux_dmabuf_v1::Request::GetDefaultFeedback { id } => {
                log::debug!("[dmabuf] GetDefaultFeedback requested");
                let feedback = data_init.init(id, DmaBufFeedbackData { for_surface: false });
                send_feedback_events(state, &feedback);
            }
            zwp_linux_dmabuf_v1::Request::GetSurfaceFeedback { id, surface } => {
                log::debug!("[dmabuf] GetSurfaceFeedback requested for surface {:?}", surface.id());
                let feedback = data_init.init(id, DmaBufFeedbackData { for_surface: true });
                send_feedback_events(state, &feedback);
            }
            _ => {}
        }
    }
}

fn send_feedback_events(state: &State, feedback: &ZwpLinuxDmabufFeedbackV1) {
    let formats = if let Some(ref renderer) = state.gpu_renderer {
        renderer.supported_formats.clone()
    } else {
        vec![
            crate::renderer::DmaBufFormat {
                format: drm_fourcc::DrmFourcc::Argb8888 as u32,
                modifier: drm_fourcc::DrmModifier::Linear.into(),
            },
            crate::renderer::DmaBufFormat {
                format: drm_fourcc::DrmFourcc::Xrgb8888 as u32,
                modifier: drm_fourcc::DrmModifier::Linear.into(),
            },
        ]
    };

    let table_size = formats.len() * std::mem::size_of::<FormatModifierEntry>();
    log::debug!(
        "[dmabuf] Sending feedback: {} formats, table_size={}",
        formats.len(),
        table_size
    );

    let fd = match create_format_table_fd(&formats) {
        Ok(fd) => fd,
        Err(e) => {
            log::error!("[dmabuf] Failed to create format table: {}", e);
            feedback.done();
            return;
        }
    };

    feedback.format_table(fd.as_fd(), table_size as u32);

    let (main_dev, scanout_dev) = if let Some(ref renderer) = state.gpu_renderer {
        (renderer.render_node_dev(), renderer.drm_dev())
    } else {
        let render_dev = std::fs::metadata("/dev/dri/renderD128")
            .or_else(|_| std::fs::metadata("/dev/dri/renderD129"))
            .map(|m| {
                use std::os::unix::fs::MetadataExt;
                m.rdev()
            })
            .unwrap_or(0);
        let card_dev = std::fs::metadata("/dev/dri/card0")
            .or_else(|_| std::fs::metadata("/dev/dri/card1"))
            .map(|m| {
                use std::os::unix::fs::MetadataExt;
                m.rdev()
            })
            .unwrap_or(0);
        (render_dev, card_dev)
    };

    log::debug!(
        "[dmabuf] Feedback devices: main_dev={} scanout_dev={}",
        main_dev,
        scanout_dev
    );

    let main_dev_bytes = main_dev.to_ne_bytes();
    feedback.main_device(main_dev_bytes.to_vec());

    let scanout_dev_bytes = scanout_dev.to_ne_bytes();
    feedback.tranche_target_device(scanout_dev_bytes.to_vec());
    feedback.tranche_flags(zwp_linux_dmabuf_feedback_v1::TrancheFlags::Scanout);

    let indices: Vec<u8> = (0..formats.len() as u16)
        .flat_map(|i| i.to_ne_bytes())
        .collect();
    feedback.tranche_formats(indices);

    feedback.tranche_done();
    feedback.done();
    log::debug!("[dmabuf] Feedback sent successfully");
}

fn create_format_table_fd(
    formats: &[crate::renderer::DmaBufFormat],
) -> Result<OwnedFd, std::io::Error> {
    use std::io::Write;

    let fd = unsafe {
        let fd = libc::memfd_create(
            c"dmabuf-format-table".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        );
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        OwnedFd::from_raw_fd(fd)
    };

    let mut file = std::fs::File::from(fd.try_clone()?);
    for fmt in formats {
        let entry = FormatModifierEntry {
            format: fmt.format,
            _padding: 0,
            modifier: fmt.modifier,
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &entry as *const FormatModifierEntry as *const u8,
                std::mem::size_of::<FormatModifierEntry>(),
            )
        };
        file.write_all(bytes)?;
    }

    unsafe {
        libc::fcntl(
            fd.as_raw_fd(),
            libc::F_ADD_SEALS,
            libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE,
        );
    }

    Ok(fd)
}

impl Dispatch<ZwpLinuxDmabufFeedbackV1, DmaBufFeedbackData> for State {
    fn request(
        _state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &ZwpLinuxDmabufFeedbackV1,
        request: zwp_linux_dmabuf_feedback_v1::Request,
        _data: &DmaBufFeedbackData,
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        if let zwp_linux_dmabuf_feedback_v1::Request::Destroy = request {}
    }
}

impl Dispatch<ZwpLinuxBufferParamsV1, DmaBufParamsData> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        resource: &ZwpLinuxBufferParamsV1,
        request: zwp_linux_buffer_params_v1::Request,
        data: &DmaBufParamsData,
        dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            zwp_linux_buffer_params_v1::Request::Add {
                fd,
                plane_idx,
                offset,
                stride,
                modifier_hi,
                modifier_lo,
            } => {
                let mut inner = data.inner.lock().unwrap();

                inner.planes.push(DmaBufPlane {
                    fd,
                    plane_idx,
                    offset,
                    stride,
                    modifier_hi,
                    modifier_lo,
                });
            }
            zwp_linux_buffer_params_v1::Request::Create {
                width,
                height,
                format,
                ..
            } => {
                let mut inner = data.inner.lock().unwrap();
                inner.width = width;
                inner.height = height;
                inner.format = format;

                let planes = std::mem::take(&mut inner.planes);

                if planes.is_empty() {
                    resource.failed();
                    return;
                }

                let buffer_data = DmaBufBufferData {
                    width,
                    height,
                    format,
                    planes,
                };

                let client = resource.client().unwrap();
                let buffer: WlBuffer = client
                    .create_resource::<WlBuffer, DmaBufBufferData, Self>(dhandle, 1, buffer_data)
                    .unwrap();

                if let Some(data) = buffer.data::<DmaBufBufferData>() {
                    if let Some(plane) = data.planes.first() {
                        let modifier =
                            ((plane.modifier_hi as u64) << 32) | (plane.modifier_lo as u64);
                        let dup_fd = plane.fd.try_clone().expect("Failed to dup dmabuf fd");

                        let planes: Vec<crate::state::DmaBufPlaneInfo> = data
                            .planes
                            .iter()
                            .map(|p| crate::state::DmaBufPlaneInfo {
                                fd: p.fd.try_clone().expect("Failed to dup plane fd"),
                                offset: p.offset,
                                stride: p.stride,
                                modifier: ((p.modifier_hi as u64) << 32) | (p.modifier_lo as u64),
                            })
                            .collect();

                        log::debug!(
                            "[dmabuf] Created buffer: {}x{} format={:#x} modifier={:#x} planes={}",
                            data.width,
                            data.height,
                            data.format,
                            modifier,
                            planes.len()
                        );

                        let info = crate::state::DmaBufBufferInfo {
                            width: data.width,
                            height: data.height,
                            format: data.format,
                            modifier,
                            fd: dup_fd,
                            stride: plane.stride,
                            offset: plane.offset,
                            planes,
                        };
                        state.dmabuf_buffers.insert(buffer.id(), info);
                    }
                }

                resource.created(&buffer);
            }
            zwp_linux_buffer_params_v1::Request::CreateImmed {
                buffer_id,
                width,
                height,
                format,
                ..
            } => {
                let mut inner = data.inner.lock().unwrap();
                inner.width = width;
                inner.height = height;
                inner.format = format;

                let planes = std::mem::take(&mut inner.planes);

                let buffer_data = DmaBufBufferData {
                    width,
                    height,
                    format,
                    planes,
                };

                let buffer: WlBuffer = data_init.init(buffer_id, buffer_data);

                if let Some(data) = buffer.data::<DmaBufBufferData>() {
                    if let Some(plane) = data.planes.first() {
                        let modifier =
                            ((plane.modifier_hi as u64) << 32) | (plane.modifier_lo as u64);
                        let dup_fd = plane.fd.try_clone().expect("Failed to dup dmabuf fd");

                        let planes: Vec<crate::state::DmaBufPlaneInfo> = data
                            .planes
                            .iter()
                            .map(|p| crate::state::DmaBufPlaneInfo {
                                fd: p.fd.try_clone().expect("Failed to dup plane fd"),
                                offset: p.offset,
                                stride: p.stride,
                                modifier: ((p.modifier_hi as u64) << 32) | (p.modifier_lo as u64),
                            })
                            .collect();

                        log::debug!("[dmabuf] CreateImmed buffer: {}x{} format={:#x} modifier={:#x} planes={} buffer_id={:?}",
                            data.width, data.height, data.format, modifier, planes.len(), buffer.id());

                        let info = crate::state::DmaBufBufferInfo {
                            width: data.width,
                            height: data.height,
                            format: data.format,
                            modifier,
                            fd: dup_fd,
                            stride: plane.stride,
                            offset: plane.offset,
                            planes,
                        };
                        state.dmabuf_buffers.insert(buffer.id(), info);
                        log::debug!(
                            "[dmabuf] Registered buffer {:?}, total dmabuf_buffers={}",
                            buffer.id(),
                            state.dmabuf_buffers.len()
                        );
                    }
                }
            }
            zwp_linux_buffer_params_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<WlBuffer, DmaBufBufferData> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        resource: &WlBuffer,
        request: wayland_server::protocol::wl_buffer::Request,
        _data: &DmaBufBufferData,
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        if let wayland_server::protocol::wl_buffer::Request::Destroy = request {
            if let Some(ref mut renderer) = state.gpu_renderer {
                renderer.remove_texture(resource.id().protocol_id() as u64);
            }
        }
    }
}
