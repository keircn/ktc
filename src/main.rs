mod state;
mod protocols;

use wayland_server::{Display, ListeningSocket, Resource};
use wayland_server::protocol::{
    wl_compositor::WlCompositor,
    wl_seat::WlSeat,
    wl_output::WlOutput,
    wl_shm::WlShm,
    wl_data_device_manager::WlDataDeviceManager,
};
use wayland_protocols::xdg::shell::server::xdg_wm_base::XdgWmBase;
use winit::event_loop::{EventLoop, ControlFlow};
use winit::event::{Event, WindowEvent};
use winit::window::Window;
use std::sync::Arc;
use std::rc::Rc;

use state::State;

fn main() {
    let mut display = Display::<State>::new().expect("Failed to create display");
    let dh = display.handle();
    
    dh.create_global::<State, WlCompositor, _>(6, ());
    dh.create_global::<State, XdgWmBase, _>(5, ());
    dh.create_global::<State, WlSeat, _>(7, ());
    dh.create_global::<State, WlOutput, _>(4, ());
    dh.create_global::<State, WlShm, _>(1, ());
    dh.create_global::<State, WlDataDeviceManager, _>(3, ());

    let socket = ListeningSocket::bind_auto("wayland", 0..32)
        .expect("Failed to create socket");
    
    println!("Listening on: {}", socket.socket_name().unwrap().to_string_lossy());

    let winit_loop = EventLoop::new().expect("Failed to create winit event loop");
    
    let window_attrs = Window::default_attributes()
        .with_title("KTC Compositor")
        .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080));
    let window = Rc::new(winit_loop.create_window(window_attrs).expect("Failed to create window"));

    let context = softbuffer::Context::new(window.clone()).expect("Failed to create softbuffer context");
    let mut surface = softbuffer::Surface::new(&context, window.clone()).expect("Failed to create surface");

    let mut calloop_loop = calloop::EventLoop::<LoopData>::try_new()
        .expect("Failed to create calloop event loop");

    let poll_fd = display.backend().poll_fd().try_clone_to_owned()
        .expect("Failed to clone poll fd");
    
    calloop_loop
        .handle()
        .insert_source(
            calloop::generic::Generic::new(
                &socket,
                calloop::Interest::READ,
                calloop::Mode::Level,
            ),
            |_, socket, data| {
                if let Some(stream) = socket.accept().ok().flatten() {
                    data.display.handle().insert_client(stream, Arc::new(()))
                        .expect("Failed to insert client");
                }
                Ok(calloop::PostAction::Continue)
            },
        )
        .expect("Failed to insert socket source");

    calloop_loop
        .handle()
        .insert_source(
            calloop::generic::Generic::new(
                poll_fd,
                calloop::Interest::READ,
                calloop::Mode::Level,
            ),
            |_, _, data| {
                data.display.dispatch_clients(&mut data.state).ok();
                data.display.flush_clients().ok();
                Ok(calloop::PostAction::Continue)
            },
        )
        .expect("Failed to insert display source");

    let mut loop_data = LoopData { 
        display,
        state: State::new(),
    };

    winit_loop.run(move |event, target| {
        target.set_control_flow(ControlFlow::Poll);
        
        calloop_loop.dispatch(Some(std::time::Duration::from_millis(1)), &mut loop_data).ok();

        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                target.exit();
            }
            Event::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                let (width, height) = {
                    let size = window.inner_size();
                    (size.width as usize, size.height as usize)
                };
                
                surface.resize(
                    std::num::NonZeroU32::new(width as u32).unwrap(),
                    std::num::NonZeroU32::new(height as u32).unwrap(),
                ).ok();

                let mut buffer = surface.buffer_mut().expect("Failed to get buffer");
                
                for pixel in buffer.iter_mut() {
                    *pixel = 0xFF202020;
                }
                
                let mut buffers_to_release = Vec::new();
                
                for (_id, surface_data) in &loop_data.state.surfaces {
                    if let Some(ref wl_buffer) = surface_data.buffer {
                        if let Some(pixels) = loop_data.state.get_buffer_pixels(wl_buffer) {
                            if let Some(buffer_data) = loop_data.state.buffers.get(&wl_buffer.id().protocol_id()) {
                                let buf_width = buffer_data.width as usize;
                                let buf_height = buffer_data.height as usize;
                                
                                for y in 0..buf_height.min(height) {
                                    for x in 0..buf_width.min(width) {
                                        let src_idx = y * buf_width + x;
                                        let dst_idx = y * width + x;
                                        if src_idx < pixels.len() && dst_idx < buffer.len() {
                                            buffer[dst_idx] = pixels[src_idx];
                                        }
                                    }
                                }
                                
                                buffers_to_release.push(wl_buffer.clone());
                            }
                        }
                    }
                }
                
                buffer.present().ok();
                
                for wl_buffer in buffers_to_release {
                    wl_buffer.release();
                }
                
                let time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u32;
                    
                for callback in loop_data.state.frame_callbacks.drain(..) {
                    callback.done(time);
                }
                
                loop_data.display.flush_clients().ok();
            }
            Event::AboutToWait => {
                window.request_redraw();
            }
            _ => {}
        }
    }).expect("Event loop error");
}

struct LoopData {
    display: Display<State>,
    state: State,
}
