use wayland_server::protocol::wl_buffer::WlBuffer;
use wayland_server::protocol::wl_shm;
use wayland_server::{Dispatch, GlobalDispatch, Resource};
use wayland_protocols_wlr::screencopy::v1::server::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
};
use crate::state::{State, ScreencopyFrameState};

impl GlobalDispatch<ZwlrScreencopyManagerV1, ()> for State {
    fn bind(
        _state: &mut Self,
        _handle: &wayland_server::DisplayHandle,
        _client: &wayland_server::Client,
        resource: wayland_server::New<ZwlrScreencopyManagerV1>,
        _global_data: &(),
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        _resource: &ZwlrScreencopyManagerV1,
        request: zwlr_screencopy_manager_v1::Request,
        _data: &(),
        _dhandle: &wayland_server::DisplayHandle,
        data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            zwlr_screencopy_manager_v1::Request::CaptureOutput {
                frame,
                overlay_cursor: _,
                output: _,
            } => {
                let (width, height) = state.screen_size();
                let frame_data = ScreencopyFrameState {
                    x: 0,
                    y: 0,
                    width,
                    height,
                    buffer: None,
                    with_damage: false,
                };
                let screencopy_frame = data_init.init(frame, frame_data);
                state.send_screencopy_buffer_info(&screencopy_frame, width, height);
            }
            zwlr_screencopy_manager_v1::Request::CaptureOutputRegion {
                frame,
                overlay_cursor: _,
                output: _,
                x,
                y,
                width,
                height,
            } => {
                let frame_data = ScreencopyFrameState {
                    x,
                    y,
                    width,
                    height,
                    buffer: None,
                    with_damage: false,
                };
                let screencopy_frame = data_init.init(frame, frame_data);
                state.send_screencopy_buffer_info(&screencopy_frame, width, height);
            }
            zwlr_screencopy_manager_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, ScreencopyFrameState> for State {
    fn request(
        state: &mut Self,
        _client: &wayland_server::Client,
        resource: &ZwlrScreencopyFrameV1,
        request: zwlr_screencopy_frame_v1::Request,
        data: &ScreencopyFrameState,
        _dhandle: &wayland_server::DisplayHandle,
        _data_init: &mut wayland_server::DataInit<'_, Self>,
    ) {
        match request {
            zwlr_screencopy_frame_v1::Request::Copy { buffer } => {
                state.queue_screencopy_frame(resource.clone(), buffer, data, false);
            }
            zwlr_screencopy_frame_v1::Request::CopyWithDamage { buffer } => {
                state.queue_screencopy_frame(resource.clone(), buffer, data, true);
            }
            zwlr_screencopy_frame_v1::Request::Destroy => {
                state.screencopy_frames.retain(|f| f.frame.id() != resource.id());
            }
            _ => {}
        }
    }
}

impl State {
    fn send_screencopy_buffer_info(&self, frame: &ZwlrScreencopyFrameV1, width: i32, height: i32) {
        let stride = width as u32 * 4;
        frame.buffer(
            wl_shm::Format::Xrgb8888.into(),
            width as u32,
            height as u32,
            stride,
        );

        if frame.version() >= 3 {
            frame.buffer_done();
        }
    }

    pub fn queue_screencopy_frame(
        &mut self,
        frame: ZwlrScreencopyFrameV1,
        buffer: WlBuffer,
        region: &ScreencopyFrameState,
        with_damage: bool,
    ) {
        self.screencopy_frames.push(PendingScreencopy {
            frame,
            buffer,
            x: region.x,
            y: region.y,
            width: region.width,
            height: region.height,
            with_damage,
        });
    }

    pub fn process_screencopy_frames(&mut self) {
        let frames = std::mem::take(&mut self.screencopy_frames);

        for pending in frames {
            if self.copy_frame_to_buffer(&pending) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap();
                let secs = now.as_secs();
                let nsecs = now.subsec_nanos();

                if pending.with_damage && pending.frame.version() >= 2 {
                    pending.frame.damage(0, 0, pending.width as u32, pending.height as u32);
                }

                pending.frame.flags(zwlr_screencopy_frame_v1::Flags::empty());
                pending.frame.ready((secs >> 32) as u32, secs as u32, nsecs);
            } else {
                pending.frame.failed();
            }
        }
    }

    fn copy_frame_to_buffer(&mut self, pending: &PendingScreencopy) -> bool {
        let buffer_id = pending.buffer.id().protocol_id();
        let buffer_data = match self.buffers.get(&buffer_id) {
            Some(data) => data.clone(),
            None => return false,
        };

        if buffer_data.width != pending.width || buffer_data.height != pending.height {
            return false;
        }

        let pool_id = buffer_data.pool_id;
        let pool_data = match self.shm_pools.get_mut(&pool_id) {
            Some(data) => data,
            None => return false,
        };

        if pool_data.mmap_ptr.is_none() {
            use std::os::fd::{AsFd, AsRawFd};
            unsafe {
                let ptr = libc::mmap(
                    std::ptr::null_mut(),
                    pool_data.size as usize,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    pool_data.fd.as_fd().as_raw_fd(),
                    0,
                );

                if ptr == libc::MAP_FAILED {
                    return false;
                }

                pool_data.mmap_ptr = std::ptr::NonNull::new(ptr as *mut u8);
            }
        }

        let mmap_ptr = match pool_data.mmap_ptr {
            Some(ptr) => ptr,
            None => return false,
        };

        let canvas_pixels = self.canvas.as_slice();
        let canvas_width = self.canvas.width as i32;
        let canvas_height = self.canvas.height as i32;
        let canvas_stride = self.canvas.stride;

        let src_x = pending.x.max(0).min(canvas_width);
        let src_y = pending.y.max(0).min(canvas_height);
        let copy_width = pending.width.min(canvas_width - src_x) as usize;
        let copy_height = pending.height.min(canvas_height - src_y) as usize;

        unsafe {
            let dst_ptr = mmap_ptr.as_ptr().add(buffer_data.offset as usize) as *mut u32;

            for row in 0..copy_height {
                let src_row = (src_y as usize + row) * canvas_stride + src_x as usize;
                let dst_row = row * pending.width as usize;

                if src_row + copy_width <= canvas_pixels.len() {
                    std::ptr::copy_nonoverlapping(
                        canvas_pixels.as_ptr().add(src_row),
                        dst_ptr.add(dst_row),
                        copy_width,
                    );
                }
            }
        }

        pending.buffer.release();
        true
    }
}

pub struct PendingScreencopy {
    pub frame: ZwlrScreencopyFrameV1,
    pub buffer: WlBuffer,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub with_damage: bool,
}
