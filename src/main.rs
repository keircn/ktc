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
use std::sync::Arc;

use state::State;

fn main() {
    let is_nested = std::env::var("WAYLAND_DISPLAY").is_ok() || std::env::var("DISPLAY").is_ok();
    
    if is_nested {
        println!("Running in nested mode (client of existing compositor)");
        run_nested();
    } else {
        println!("Running in standalone mode (native compositor)");
        run_standalone();
    }
}

fn setup_wayland() -> (Display<State>, ListeningSocket) {
    let display = Display::<State>::new().expect("Failed to create display");
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
    
    (display, socket)
}

fn run_nested() {
    use winit::event_loop::{EventLoop, ControlFlow};
    use winit::event::{Event, WindowEvent};
    use winit::window::Window;
    use std::rc::Rc;

    let (mut display, socket) = setup_wayland();

    let winit_loop = EventLoop::new().expect("Failed to create winit event loop");
    
    let window_attrs = Window::default_attributes()
        .with_title("KTC Compositor (Nested)")
        .with_inner_size(winit::dpi::LogicalSize::new(1920, 1080));
    let window = Rc::new(winit_loop.create_window(window_attrs).expect("Failed to create window"));

    let context = softbuffer::Context::new(window.clone()).expect("Failed to create softbuffer context");
    let mut surface = softbuffer::Surface::new(&context, window.clone()).expect("Failed to create surface");

    let mut calloop_loop = calloop::EventLoop::<NestedLoopData>::try_new()
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

    let mut loop_data = NestedLoopData { 
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
                render_frame(&mut surface, &window, &mut loop_data);
            }
            Event::AboutToWait => {
                window.request_redraw();
            }
            _ => {}
        }
    }).expect("Event loop error");
}

fn run_standalone() {
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;
    use nix::sys::mman::{mmap, MapFlags, ProtFlags};
    
    let (mut display, socket) = setup_wayland();

    let fb_path = if std::path::Path::new("/dev/fb0").exists() {
        "/dev/fb0"
    } else {
        eprintln!("No framebuffer device found at /dev/fb0");
        eprintln!("Running in headless mode (no display output)");
        eprintln!("To see client windows, run this compositor in nested mode instead.");
        ""
    };

    let fb_info = if !fb_path.is_empty() {
        match OpenOptions::new().read(true).write(true).open(fb_path) {
            Ok(fb_file) => {
                let fd = fb_file.as_raw_fd();
                match get_fb_info(fd) {
                    Ok((width, height, line_length, bpp)) => {
                        eprintln!("Framebuffer: {}x{} @ {}bpp", width, height, bpp);
                        let size = (line_length * height) as usize;
                        
                        let fb_ptr = unsafe {
                            mmap(
                                None,
                                size.try_into().unwrap(),
                                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                                MapFlags::MAP_SHARED,
                                fd,
                                0,
                            ).ok()
                        };
                        
                        if let Some(ptr) = fb_ptr {
                            Some(FramebufferInfo {
                                ptr: ptr as *mut u32,
                                width: width as usize,
                                height: height as usize,
                                line_length: line_length as usize,
                                _file: fb_file,
                            })
                        } else {
                            eprintln!("Failed to mmap framebuffer, running headless");
                            None
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to get framebuffer info: {}, running headless", e);
                        None
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to open framebuffer: {}, running headless", e);
                None
            }
        }
    } else {
        None
    };

    let mut calloop_loop = calloop::EventLoop::<StandaloneLoopData>::try_new()
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

    let _timer = calloop_loop.handle()
        .insert_source(
            calloop::timer::Timer::immediate(),
            |_deadline, _: &mut (), data| {
                render_standalone(&mut data.state, &mut data.display, data.fb_info.as_ref());
                calloop::timer::TimeoutAction::ToDuration(std::time::Duration::from_millis(16))
            },
        )
        .expect("Failed to insert timer");

    let mut loop_data = StandaloneLoopData {
        display,
        state: State::new(),
        fb_info,
    };

    println!("Compositor running in standalone mode. Press Ctrl+C to exit.");
    
    loop {
        calloop_loop.dispatch(None, &mut loop_data).expect("Event loop error");
    }
}

fn render_frame(
    surface: &mut softbuffer::Surface<std::rc::Rc<winit::window::Window>, std::rc::Rc<winit::window::Window>>,
    window: &winit::window::Window,
    loop_data: &mut NestedLoopData
) {
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

fn render_standalone(state: &mut State, display: &mut Display<State>, fb_info: Option<&FramebufferInfo>) {
    if let Some(fb) = fb_info {
        unsafe {
            let pixels = std::slice::from_raw_parts_mut(fb.ptr, fb.width * fb.height);
            
            for pixel in pixels.iter_mut() {
                *pixel = 0xFF202020;
            }
            
            for (_id, surface_data) in &state.surfaces {
                if let Some(ref wl_buffer) = surface_data.buffer {
                    if let Some(client_pixels) = state.get_buffer_pixels(wl_buffer) {
                        if let Some(buffer_data) = state.buffers.get(&wl_buffer.id().protocol_id()) {
                            let buf_width = buffer_data.width as usize;
                            let buf_height = buffer_data.height as usize;
                            
                            for y in 0..buf_height.min(fb.height) {
                                for x in 0..buf_width.min(fb.width) {
                                    let src_idx = y * buf_width + x;
                                    let dst_idx = y * fb.width + x;
                                    if src_idx < client_pixels.len() && dst_idx < pixels.len() {
                                        pixels[dst_idx] = client_pixels[src_idx];
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    let mut buffers_to_release = Vec::new();
    
    for (_id, surface_data) in &state.surfaces {
        if let Some(ref wl_buffer) = surface_data.buffer {
            buffers_to_release.push(wl_buffer.clone());
        }
    }
    
    for wl_buffer in buffers_to_release {
        wl_buffer.release();
    }
    
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u32;
        
    for callback in state.frame_callbacks.drain(..) {
        callback.done(time);
    }
    
    display.flush_clients().ok();
}

struct NestedLoopData {
    display: Display<State>,
    state: State,
}

struct StandaloneLoopData {
    display: Display<State>,
    state: State,
    fb_info: Option<FramebufferInfo>,
}

struct FramebufferInfo {
    ptr: *mut u32,
    width: usize,
    height: usize,
    line_length: usize,
    _file: std::fs::File,
}

unsafe impl Send for FramebufferInfo {}

fn get_fb_info(fd: std::os::fd::RawFd) -> Result<(u32, u32, u32, u32), String> {
    use std::mem::MaybeUninit;
    
    #[repr(C)]
    struct FbVarScreeninfo {
        xres: u32,
        yres: u32,
        xres_virtual: u32,
        yres_virtual: u32,
        xoffset: u32,
        yoffset: u32,
        bits_per_pixel: u32,
        grayscale: u32,
        _padding: [u8; 160],
    }
    
    #[repr(C)]
    struct FbFixScreeninfo {
        id: [u8; 16],
        smem_start: usize,
        smem_len: u32,
        type_: u32,
        type_aux: u32,
        visual: u32,
        xpanstep: u16,
        ypanstep: u16,
        ywrapstep: u16,
        line_length: u32,
        _padding: [u8; 216],
    }
    
    const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
    const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;
    
    unsafe {
        let mut vinfo = MaybeUninit::<FbVarScreeninfo>::zeroed();
        let mut finfo = MaybeUninit::<FbFixScreeninfo>::zeroed();
        
        if libc::ioctl(fd, FBIOGET_VSCREENINFO, vinfo.as_mut_ptr()) < 0 {
            return Err("Failed to get variable screen info".to_string());
        }
        
        if libc::ioctl(fd, FBIOGET_FSCREENINFO, finfo.as_mut_ptr()) < 0 {
            return Err("Failed to get fixed screen info".to_string());
        }
        
        let vinfo = vinfo.assume_init();
        let finfo = finfo.assume_init();
        
        Ok((vinfo.xres, vinfo.yres, finfo.line_length / 4, vinfo.bits_per_pixel))
    }
}
