mod state;
mod protocols;
mod input;
mod logging;

use clap::{Parser, Subcommand};
use input::KeyState;
use wayland_server::protocol::wl_keyboard::KeyState as WlKeyState;
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

#[derive(Parser)]
#[command(name = "ktc")]
#[command(about = "KTC - Minimal Wayland Tiling Compositor", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Start the compositor session")]
    Start {
        #[arg(short, long, help = "Force nested mode (run inside existing compositor)")]
        nested: bool,
        
        #[arg(short, long, help = "Force standalone mode (native DRM/KMS)")]
        standalone: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    
    match cli.command {
        Some(Commands::Start { nested, standalone }) => {
            if unsafe { libc::geteuid() } == 0 {
                eprintln!("Error: KTC must not be run as root");
                eprintln!("Add your user to the 'video' and 'input' groups instead:");
                eprintln!("  sudo usermod -aG video,input $USER");
                eprintln!("Then log out and back in.");
                std::process::exit(1);
            }
            
            logging::FileLogger::init().expect("Failed to initialize logging");
            
            if nested && standalone {
                eprintln!("Error: Cannot specify both --nested and --standalone");
                std::process::exit(1);
            }
            
            let is_nested = if nested {
                true
            } else if standalone {
                false
            } else {
                std::env::var("WAYLAND_DISPLAY")
                    .ok()
                    .filter(|v| !v.is_empty())
                    .is_some() 
                    || std::env::var("DISPLAY")
                        .ok()
                        .filter(|v| !v.is_empty())
                        .is_some()
            };
            
            if is_nested {
                log::info!("Running in nested mode (client of existing compositor)");
                run_nested();
            } else {
                log::info!("Running in standalone mode (native compositor)");
                run_standalone();
            }
        }
        None => {
            print_help();
        }
    }
}

fn print_help() {
    println!("KTC - Minimal Wayland Tiling Compositor\n");
    println!("USAGE:");
    println!("    ktc start [OPTIONS]    Start the compositor session");
    println!();
    println!("OPTIONS:");
    println!("    -n, --nested          Force nested mode (run inside existing compositor)");
    println!("    -s, --standalone      Force standalone mode (native DRM/KMS)");
    println!();
    println!("EXAMPLES:");
    println!("    ktc start             Auto-detect mode and start compositor");
    println!("    ktc start --nested    Start in nested mode for testing");
    println!("    ktc start             Start from TTY as native compositor (DO NOT use sudo)");
    println!();
    println!("SETUP (for standalone mode from TTY):");
    println!("    sudo usermod -aG video $USER");
    println!("    sudo usermod -aG input $USER");
    println!("    Log out and back in for group changes to take effect");
    println!();
    println!("KEYBINDS:");
    println!("    Ctrl+Alt+Q           Exit compositor");
    println!("    Alt+T                Launch terminal (ghostty)");
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
    
    log::info!("Listening on: {}", socket.socket_name().unwrap().to_string_lossy());
    
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
                    log::info!("New client connecting to Wayland socket");
                    match data.display.handle().insert_client(stream, Arc::new(())) {
                        Ok(client_id) => {
                            log::info!("Client connected successfully: {:?}", client_id);
                        }
                        Err(e) => {
                            log::error!("Failed to insert client: {}", e);
                        }
                    }
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
    
    loop_data.state.set_screen_size(1920, 1080);

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
    use input::{InputHandler, InputAction};
    
    let _tty_guard = setup_tty();
    
    let (mut display, socket) = setup_wayland();
    
    let socket_name = socket.socket_name()
        .expect("Failed to get socket name")
        .to_string_lossy()
        .to_string();

    let drm_device = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/dri/card0")
        .or_else(|_| OpenOptions::new().read(true).write(true).open("/dev/dri/card1"));

    let drm_info = match drm_device {
        Ok(device) => {
            log::info!("Opened DRM device");
            match setup_drm(&device) {
                Ok(info) => {
                    log::info!("DRM setup complete: {}x{}", info.width, info.height);
                    Some(info)
                }
                Err(e) => {
                    log::error!("Failed to setup DRM: {}", e);
                    log::warn!("Running in headless mode");
                    None
                }
            }
        }
        Err(e) => {
            log::error!("Failed to open DRM device: {}", e);
            log::warn!("Running in headless mode (no display output)");
            log::info!("Make sure you're in the 'video' group: sudo usermod -aG video $USER");
            None
        }
    };

    let input_handler = match InputHandler::new() {
        Ok(handler) => {
            log::info!("Input handler initialized");
            Some(handler)
        }
        Err(e) => {
            log::error!("Failed to initialize input handler: {}", e);
            log::warn!("Input will not be available");
            log::info!("Make sure you're in the 'input' group: sudo usermod -aG input $USER");
            None
        }
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
                    log::info!("New client connecting to Wayland socket");
                    match data.display.handle().insert_client(stream, Arc::new(())) {
                        Ok(client_id) => {
                            log::info!("Client connected successfully: {:?}", client_id);
                        }
                        Err(e) => {
                            log::error!("Failed to insert client: {}", e);
                        }
                    }
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

    if let Some(ref handler) = input_handler {
        let input_fd = handler.as_fd().try_clone_to_owned()
            .expect("Failed to clone input fd");
        
        calloop_loop
            .handle()
            .insert_source(
                calloop::generic::Generic::new(
                    input_fd,
                    calloop::Interest::READ,
                    calloop::Mode::Level,
                ),
                |_, _, data| {
                    if let Some(ref mut handler) = data.input_handler {
                        handler.dispatch().ok();
                        let mut should_exit = false;
                        
                        handler.process_events(|action| {
                            match action {
                                InputAction::ExitCompositor => {
                                    log::info!("Ctrl+Alt+Q pressed - exiting compositor");
                                    should_exit = true;
                                }
                                InputAction::LaunchTerminal => {
                                    log::info!("Alt+T pressed - launching ghostty terminal");
                                    
                                    let xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR")
                                        .unwrap_or_else(|_| "/tmp".to_string());
                                    
                                    match std::process::Command::new("ghostty")
                                        .env("WAYLAND_DISPLAY", &data.socket_name)
                                        .env("XDG_RUNTIME_DIR", xdg_runtime_dir)
                                        .spawn() {
                                        Ok(child) => {
                                            log::info!("ghostty launched with PID {} on {}", child.id(), data.socket_name);
                                        }
                                        Err(e) => {
                                            log::error!("Failed to launch ghostty: {}", e);
                                            log::info!("Make sure ghostty is installed and in PATH");
                                        }
                                    }
                                }
                                InputAction::KeyEvent { keycode, state: key_state } => {
                                    let focused_id = data.state.get_focused_window().map(|w| w.id);
                                    
                                    if let Some(window_id) = focused_id {
                                        let wl_state = match key_state {
                                            KeyState::Pressed => WlKeyState::Pressed,
                                            KeyState::Released => WlKeyState::Released,
                                        };
                                        
                                        let serial = data.state.next_keyboard_serial();
                                        for keyboard in &data.state.keyboards {
                                            log::info!("Forwarding key event to window {}: keycode={}, state={:?}, serial={}", 
                                                window_id, keycode, wl_state, serial);
                                            keyboard.key(serial, 0, keycode, wl_state);
                                        }
                                        data.display.flush_clients().ok();
                                    }
                                }
                            }
                        });
                        
                        if should_exit {
                            std::process::exit(0);
                        }
                    }
                    Ok(calloop::PostAction::Continue)
                },
            )
            .expect("Failed to insert input source");
    }

    let _timer = calloop_loop.handle()
        .insert_source(
            calloop::timer::Timer::immediate(),
            |_deadline, _: &mut (), data| {
                render_standalone(&mut data.state, &mut data.display, data.drm_info.as_mut());
                calloop::timer::TimeoutAction::ToDuration(std::time::Duration::from_millis(16))
            },
        )
        .expect("Failed to insert timer");

    let mut loop_data = StandaloneLoopData {
        display,
        state: State::new(),
        drm_info,
        input_handler,
        socket_name,
    };

    log::info!("Compositor running in standalone mode. Press Ctrl+Alt+Q to exit.");
    
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
    
    for window_obj in &loop_data.state.windows {
        if !window_obj.mapped {
            continue;
        }
        
        if let Some(ref wl_buffer) = window_obj.buffer {
            if let Some(pixels) = loop_data.state.get_buffer_pixels(wl_buffer) {
                if let Some(buffer_data) = loop_data.state.buffers.get(&wl_buffer.id().protocol_id()) {
                    let buf_width = buffer_data.width as usize;
                    let buf_height = buffer_data.height as usize;
                    let win_x = window_obj.geometry.x.max(0) as usize;
                    let win_y = window_obj.geometry.y.max(0) as usize;
                    
                    for y in 0..buf_height.min(height.saturating_sub(win_y)) {
                        let dst_y = win_y + y;
                        if dst_y >= height {
                            break;
                        }
                        for x in 0..buf_width.min(width.saturating_sub(win_x)) {
                            let dst_x = win_x + x;
                            if dst_x >= width {
                                break;
                            }
                            let src_idx = y * buf_width + x;
                            let dst_idx = dst_y * width + dst_x;
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

fn render_standalone(state: &mut State, display: &mut Display<State>, drm_info: Option<&mut DrmInfo>) {
    if let Some(drm) = drm_info {
        unsafe {
            let pixels = std::slice::from_raw_parts_mut(drm.fb_ptr, drm.width * drm.height);
            
            pixels.fill(0xFF202020);
            
            for window in &state.windows {
                if !window.mapped {
                    continue;
                }
                
                if let Some(ref wl_buffer) = window.buffer {
                    if let Some(client_pixels) = state.get_buffer_pixels(wl_buffer) {
                        if let Some(buffer_data) = state.buffers.get(&wl_buffer.id().protocol_id()) {
                            let buf_width = buffer_data.width as usize;
                            let buf_height = buffer_data.height as usize;
                            let win_x = window.geometry.x.max(0) as usize;
                            let win_y = window.geometry.y.max(0) as usize;
                            
                            for y in 0..buf_height.min(drm.height.saturating_sub(win_y)) {
                                let dst_y = win_y + y;
                                if dst_y >= drm.height {
                                    break;
                                }
                                let src_offset = y * buf_width;
                                let dst_offset = dst_y * drm.width + win_x;
                                let copy_width = buf_width.min(drm.width.saturating_sub(win_x));
                                
                                if src_offset + copy_width <= client_pixels.len() && 
                                   dst_offset + copy_width <= pixels.len() {
                                    std::ptr::copy_nonoverlapping(
                                        client_pixels.as_ptr().add(src_offset),
                                        pixels.as_mut_ptr().add(dst_offset),
                                        copy_width
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    for window in &state.windows {
        if let Some(ref wl_buffer) = window.buffer {
            wl_buffer.release();
        }
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
    drm_info: Option<DrmInfo>,
    input_handler: Option<input::InputHandler>,
    socket_name: String,
}

struct DrmInfo {
    _device: std::fs::File,
    _mapping: drm::control::dumbbuffer::DumbMapping<'static>,
    fb_ptr: *mut u32,
    width: usize,
    height: usize,
    fb_id: u32,
    _crtc: drm::control::crtc::Handle,
}

unsafe impl Send for DrmInfo {}

fn setup_drm(device: &std::fs::File) -> Result<DrmInfo, Box<dyn std::error::Error>> {
    use drm::control::{Device as ControlDevice, connector};
    use std::os::fd::{AsFd, BorrowedFd};
    
    struct Card(std::fs::File);
    
    impl AsFd for Card {
        fn as_fd(&self) -> BorrowedFd<'_> {
            self.0.as_fd()
        }
    }
    
    impl drm::Device for Card {}
    impl ControlDevice for Card {}
    
    let card = Card(device.try_clone()?);
    
    let res = card.resource_handles()?;
    let connectors: Vec<_> = res.connectors().iter()
        .filter_map(|&conn| card.get_connector(conn, true).ok())
        .collect();
    
    let connector = connectors.iter()
        .find(|c| c.state() == connector::State::Connected)
        .ok_or("No connected display found")?;
    
    let mode = connector.modes().first()
        .ok_or("No display mode available")?;
    
    let (width, height) = mode.size();
    log::info!("Using display mode: {}x{}", width, height);
    
    let crtc_handle = res.crtcs().first()
        .copied()
        .ok_or("No CRTC available")?;
    
    let db = card.create_dumb_buffer((width.into(), height.into()), drm::buffer::DrmFourcc::Xrgb8888, 32)?;
    
    let fb_handle = card.add_framebuffer(&db, 24, 32)?;
    
    card.set_crtc(crtc_handle, Some(fb_handle), (0, 0), &[connector.handle()], Some(*mode))?;
    
    let db_leaked: &'static mut drm::control::dumbbuffer::DumbBuffer = Box::leak(Box::new(db));
    
    let map_handle = card.map_dumb_buffer(db_leaked)?;
    let fb_ptr = map_handle.as_ptr() as *mut u32;
    
    Ok(DrmInfo {
        _device: card.0,
        _mapping: map_handle,
        fb_ptr,
        width: width as usize,
        height: height as usize,
        fb_id: fb_handle.into(),
        _crtc: crtc_handle,
    })
}

struct TtyGuard {
    fd: std::os::fd::RawFd,
    old_mode: i32,
}

impl Drop for TtyGuard {
    fn drop(&mut self) {
        unsafe {
            libc::ioctl(self.fd, 0x4B3A, self.old_mode);
            libc::close(self.fd);
        }
        log::info!("TTY restored to text mode");
    }
}

fn setup_tty() -> Option<TtyGuard> {
    use std::os::fd::AsRawFd;
    
    let tty_path = std::fs::read_to_string("/sys/class/tty/tty0/active")
        .ok()
        .and_then(|s| {
            let tty_name = s.trim();
            if !tty_name.is_empty() {
                Some(format!("/dev/{}", tty_name))
            } else {
                None
            }
        })
        .unwrap_or_else(|| "/dev/tty".to_string());
    
    log::info!("Attempting to open TTY: {}", tty_path);
    
    let tty = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&tty_path) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("Failed to open TTY {}: {}", tty_path, e);
            return None;
        }
    };
    
    let fd = tty.as_raw_fd();
    
    unsafe {
        let mut old_mode: i32 = 0;
        if libc::ioctl(fd, 0x4B3B, &mut old_mode as *mut i32) < 0 {
            log::warn!("Failed to get TTY mode (KDGETMODE)");
            return None;
        }
        
        const KD_GRAPHICS: i32 = 0x01;
        if libc::ioctl(fd, 0x4B3A, KD_GRAPHICS) < 0 {
            log::warn!("Failed to set TTY to graphics mode (KDSETMODE)");
            return None;
        }
        
        log::info!("TTY switched to graphics mode (old_mode={})", old_mode);
        
        std::mem::forget(tty);
        
        Some(TtyGuard { fd, old_mode })
    }
}
