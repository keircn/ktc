mod state;
mod protocols;
mod input;
mod logging;
mod session;

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
    
    let initial_size = window.inner_size();
    loop_data.state.add_output(
        "nested".to_string(),
        initial_size.width as i32,
        initial_size.height as i32
    );

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
    
    let _session = match session::Session::new() {
        Ok(s) => {
            log::info!("Session initialized on VT{}", s.vt_num());
            Some(s)
        }
        Err(e) => {
            log::warn!("Failed to initialize session: {}", e);
            log::warn!("TTY may not be properly restored on exit");
            None
        }
    };
    
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
                        
                        handler.process_events(|action| {
                            match action {
                                InputAction::ExitCompositor => {
                                    log::info!("Ctrl+Alt+Q pressed - initiating shutdown");
                                    session::request_shutdown();
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
                                            let pid = child.id();
                                            log::info!("ghostty launched with PID {} on {}", pid, data.socket_name);
                                            session::register_child(pid);
                                        }
                                        Err(e) => {
                                            log::error!("Failed to launch ghostty: {}", e);
                                            log::info!("Make sure ghostty is installed and in PATH");
                                        }
                                    }
                                }
                                InputAction::FocusNext => {
                                    data.state.focus_next();
                                    data.display.flush_clients().ok();
                                }
                                InputAction::FocusPrev => {
                                    data.state.focus_prev();
                                    data.display.flush_clients().ok();
                                }
                                InputAction::KeyEvent { keycode, state: key_state, mods_depressed, mods_latched, mods_locked, group } => {
                                    let focused_keyboards = data.state.get_focused_keyboards();
                                    
                                    if !focused_keyboards.is_empty() {
                                        let wl_state = match key_state {
                                            KeyState::Pressed => WlKeyState::Pressed,
                                            KeyState::Released => WlKeyState::Released,
                                        };
                                        
                                        let serial = data.state.next_keyboard_serial();
                                        for keyboard in focused_keyboards {
                                            keyboard.key(serial, 0, keycode, wl_state);
                                            keyboard.modifiers(serial, mods_depressed, mods_latched, mods_locked, group);
                                        }
                                        data.display.flush_clients().ok();
                                    }
                                }
                            }
                        });
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
    
    if let Some(ref drm) = loop_data.drm_info {
        use state::OutputConfig;
        let output_id = loop_data.state.add_output(
            drm.name.clone(),
            drm.width as i32,
            drm.height as i32
        );
        
        loop_data.state.configure_output(output_id, OutputConfig {
            make: Some("DRM".to_string()),
            model: Some(drm.name.clone()),
            physical_size: Some((drm.physical_width as i32, drm.physical_height as i32)),
            refresh: Some(drm.refresh),
            ..Default::default()
        });
        
        log::info!("Configured output {} at {}x{}", drm.name, drm.width, drm.height);
    } else {
        loop_data.state.add_output("headless".to_string(), 1366, 768);
    }

    log::info!("Compositor running in standalone mode. Press Ctrl+Alt+Q to exit.");
    
    while session::is_running() {
        calloop_loop.dispatch(Some(std::time::Duration::from_millis(16)), &mut loop_data)
            .expect("Event loop error");
    }
    
    log::info!("Main loop exited, cleaning up...");
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
    
    loop_data.state.canvas.resize(width, height);
    if loop_data.state.screen_size() != (width as i32, height as i32) {
        loop_data.state.set_screen_size(width as i32, height as i32);
    }
    
    surface.resize(
        std::num::NonZeroU32::new(width as u32).unwrap(),
        std::num::NonZeroU32::new(height as u32).unwrap(),
    ).ok();

    loop_data.state.canvas.clear_with_pattern();
    
    let mut buffers_to_release = Vec::new();
    let focused_id = loop_data.state.focused_window;
    
    let window_infos: Vec<_> = loop_data.state.windows.iter()
        .filter(|w| w.mapped)
        .filter_map(|w| {
            let wl_buffer = w.buffer.clone()?;
            let buffer_id = wl_buffer.id().protocol_id();
            let buffer_data = loop_data.state.buffers.get(&buffer_id)?;
            let is_focused = focused_id == Some(w.id);
            Some((w.id, w.geometry, wl_buffer, buffer_data.width as usize, buffer_data.height as usize, is_focused))
        })
        .collect();
    
    for (_, geometry, wl_buffer, buf_width, buf_height, _) in &window_infos {
        if let Some(pixels) = loop_data.state.get_buffer_pixels(wl_buffer) {
            let pixels_copy: Vec<u32> = pixels.to_vec();
            loop_data.state.canvas.blit_fast(&pixels_copy, *buf_width, *buf_height, geometry.x, geometry.y);
            buffers_to_release.push(wl_buffer.clone());
        }
    }
    
    for (_, geometry, _, _, _, is_focused) in &window_infos {
        let border_color = if *is_focused { 0xFF4A9EFF } else { 0xFF404040 };
        let thickness = if *is_focused { 3 } else { 1 };
        loop_data.state.canvas.draw_border(geometry.x, geometry.y, geometry.width, geometry.height, border_color, thickness);
    }
    
    let mut buffer = surface.buffer_mut().expect("Failed to get buffer");
    buffer.copy_from_slice(loop_data.state.canvas.as_slice());
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
    state.canvas.clear_with_pattern();

    let focused_id = state.focused_window;
    
    let window_infos: Vec<_> = state.windows.iter()
        .filter(|w| w.mapped)
        .filter_map(|w| {
            let wl_buffer = w.buffer.clone()?;
            let buffer_id = wl_buffer.id().protocol_id();
            let buffer_data = state.buffers.get(&buffer_id)?;
            let is_focused = focused_id == Some(w.id);
            Some((w.geometry, wl_buffer, buffer_data.width as usize, buffer_data.height as usize, is_focused))
        })
        .collect();
    
    for (geometry, wl_buffer, buf_width, buf_height, _) in &window_infos {
        if let Some(client_pixels) = state.get_buffer_pixels(&wl_buffer) {
            let pixels_copy: Vec<u32> = client_pixels.to_vec();
            state.canvas.blit_fast(&pixels_copy, *buf_width, *buf_height, geometry.x, geometry.y);
        }
    }
    
    for (geometry, _, _, _, is_focused) in &window_infos {
        let border_color = if *is_focused { 0xFF4A9EFF } else { 0xFF404040 };
        let thickness = if *is_focused { 3 } else { 1 };
        state.canvas.draw_border(geometry.x, geometry.y, geometry.width, geometry.height, border_color, thickness);
    }
    
    if let Some(drm) = drm_info {
        unsafe {
            let fb_pixels = std::slice::from_raw_parts_mut(drm.fb_ptr, drm.width * drm.height);
            let canvas_pixels = state.canvas.as_slice();
            let copy_height = state.canvas.height.min(drm.height);
            let copy_width = state.canvas.width.min(drm.width);
            
            for y in 0..copy_height {
                let src_offset = y * state.canvas.stride;
                let dst_offset = y * drm.width;
                
                if src_offset + copy_width <= canvas_pixels.len() && 
                   dst_offset + copy_width <= fb_pixels.len() {
                    std::ptr::copy_nonoverlapping(
                        canvas_pixels.as_ptr().add(src_offset),
                        fb_pixels.as_mut_ptr().add(dst_offset),
                        copy_width
                    );
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
    physical_width: u32,
    physical_height: u32,
    refresh: i32,
    name: String,
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
    
    let connector_name = format!("{:?}-{}", connector.interface(), connector.interface_id());
    
    let mode = connector.modes().first()
        .ok_or("No display mode available")?;
    
    let (width, height) = mode.size();
    let refresh = mode.vrefresh() as i32 * 1000;
    
    let (phys_width, phys_height) = connector.size().unwrap_or((0, 0));
    
    log::info!("Using display mode: {}x{} @{}Hz (physical: {}x{}mm) on {}", 
        width, height, mode.vrefresh(), phys_width, phys_height, connector_name);
    
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
        physical_width: phys_width,
        physical_height: phys_height,
        refresh,
        name: connector_name,
    })
}
