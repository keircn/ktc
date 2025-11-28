mod state;
mod protocols;
mod input;
mod logging;
mod session;
mod config;

use clap::{Parser, Subcommand};
use config::Config;
use input::KeyState;
use wayland_server::protocol::wl_keyboard::KeyState as WlKeyState;
use wayland_server::{Display, ListeningSocket, Resource};
use wayland_server::protocol::{
    wl_compositor::WlCompositor,
    wl_seat::WlSeat,
    wl_output::WlOutput,
    wl_shm::WlShm,
    wl_data_device_manager::WlDataDeviceManager,
    wl_subcompositor::WlSubcompositor,
};
use wayland_protocols::xdg::shell::server::xdg_wm_base::XdgWmBase;
use wayland_protocols::xdg::xdg_output::zv1::server::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols_wlr::screencopy::v1::server::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;
use std::sync::Arc;
use state::{State, TITLE_BAR_HEIGHT};

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
            
            let config = Config::load();
            log::debug!("Config: {:?}", config);
            
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
                run_nested(config);
            } else {
                log::info!("Running in standalone mode (native compositor)");
                run_standalone(config);
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
    println!("    Alt+T                Launch terminal (foot)");
    println!("    Alt+Tab / Alt+J      Focus next window");
    println!("    Alt+K                Focus previous window");
}

fn setup_wayland() -> (Display<State>, ListeningSocket) {
    let display = Display::<State>::new().expect("Failed to create display");
    let dh = display.handle();
    
    dh.create_global::<State, WlCompositor, _>(6, ());
    dh.create_global::<State, WlSubcompositor, _>(1, ());
    dh.create_global::<State, XdgWmBase, _>(5, ());
    dh.create_global::<State, WlSeat, _>(7, ());
    dh.create_global::<State, WlOutput, _>(4, ());
    dh.create_global::<State, WlShm, _>(1, ());
    dh.create_global::<State, WlDataDeviceManager, _>(3, ());
    dh.create_global::<State, ZxdgOutputManagerV1, _>(3, ());
    dh.create_global::<State, ZwlrScreencopyManagerV1, _>(3, ());

    let socket = ListeningSocket::bind_auto("wayland", 0..32)
        .expect("Failed to create socket");
    
    log::info!("Listening on: {}", socket.socket_name().unwrap().to_string_lossy());
    
    (display, socket)
}

fn run_nested(config: Config) {
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
        state: State::new(config),
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
            Event::WindowEvent { event: WindowEvent::CursorMoved { position, .. }, .. } => {
                loop_data.state.handle_pointer_motion(position.x, position.y);
                loop_data.display.flush_clients().ok();
            }
            Event::WindowEvent { event: WindowEvent::MouseInput { state: button_state, button, .. }, .. } => {
                let btn = match button {
                    winit::event::MouseButton::Left => 0x110,
                    winit::event::MouseButton::Right => 0x111,
                    winit::event::MouseButton::Middle => 0x112,
                    winit::event::MouseButton::Back => 0x113,
                    winit::event::MouseButton::Forward => 0x114,
                    winit::event::MouseButton::Other(n) => 0x110 + n as u32,
                };
                let pressed = button_state == winit::event::ElementState::Pressed;
                loop_data.state.handle_pointer_button(btn, pressed);
                loop_data.display.flush_clients().ok();
            }
            Event::WindowEvent { event: WindowEvent::MouseWheel { delta, .. }, .. } => {
                let (h, v) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(h, v) => (h as f64 * 10.0, v as f64 * 10.0),
                    winit::event::MouseScrollDelta::PixelDelta(pos) => (pos.x, pos.y),
                };
                loop_data.state.handle_pointer_axis(h, -v);
                loop_data.display.flush_clients().ok();
            }
            Event::AboutToWait => {
                window.request_redraw();
            }
            _ => {}
        }
    }).expect("Event loop error");
}

fn run_standalone(config: Config) {
    use std::fs::OpenOptions;
    use input::InputHandler;
    
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
                    data.input_pending = true;
                    Ok(calloop::PostAction::Continue)
                },
            )
            .expect("Failed to insert input source");
    }

    let _timer = calloop_loop.handle()
        .insert_source(
            calloop::timer::Timer::immediate(),
            |_deadline, _: &mut (), data| {
                let frame_start = std::time::Instant::now();
                
                let input_start = std::time::Instant::now();
                if data.input_pending {
                    data.input_pending = false;
                    process_input_frame(data);
                }
                let input_time = input_start.elapsed().as_micros() as u64;
                
                let render_start = std::time::Instant::now();
                render_standalone(&mut data.state, &mut data.display, data.drm_info.as_mut());
                let render_time = render_start.elapsed().as_micros() as u64;
                
                let total_time = frame_start.elapsed().as_micros() as u64;
                data.frame_profiler.record_frame(input_time, render_time, total_time, &data.state);
                
                calloop::timer::TimeoutAction::ToDuration(std::time::Duration::from_millis(16))
            },
        )
        .expect("Failed to insert timer");

    let mut loop_data = StandaloneLoopData {
        display,
        state: State::new(config),
        drm_info,
        input_handler,
        socket_name,
        input_pending: false,
        frame_profiler: FrameProfiler::new(),
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
    if loop_data.state.needs_relayout {
        loop_data.state.needs_relayout = false;
        loop_data.state.relayout_windows();
        loop_data.display.flush_clients().ok();
    }
    
    let (width, height) = {
        let size = window.inner_size();
        (size.width as usize, size.height as usize)
    };
    
    if loop_data.state.canvas.width != width || loop_data.state.canvas.height != height {
        loop_data.state.canvas.resize(width, height);
        loop_data.state.damage_tracker.mark_full_damage();
    }
    if loop_data.state.screen_size() != (width as i32, height as i32) {
        loop_data.state.set_screen_size(width as i32, height as i32);
    }
    
    let has_pending_screencopy = !loop_data.state.screencopy_frames.is_empty();
    let has_frame_callbacks = !loop_data.state.frame_callbacks.is_empty();
    let has_damage = loop_data.state.damage_tracker.has_damage();
    let cursor_only = loop_data.state.damage_tracker.is_cursor_only();
    
    if !has_damage && !has_pending_screencopy && !has_frame_callbacks {
        return;
    }
    
    surface.resize(
        std::num::NonZeroU32::new(width as u32).unwrap(),
        std::num::NonZeroU32::new(height as u32).unwrap(),
    ).ok();

    if has_damage {
        if cursor_only {
            loop_data.state.canvas.restore_cursor();
            if loop_data.state.cursor_visible {
                loop_data.state.canvas.draw_cursor(loop_data.state.cursor_x, loop_data.state.cursor_y);
            }
        } else {
            loop_data.state.canvas.restore_cursor();
            
            let focused_id = loop_data.state.focused_window;
            
            let windows_to_render: Vec<_> = loop_data.state.windows.iter()
                .filter(|w| w.mapped && w.buffer.is_some())
                .map(|w| w.id)
                .collect();
            
            for id in &windows_to_render {
                loop_data.state.update_window_pixel_cache(*id);
            }

            loop_data.state.canvas.clear_with_pattern();
            
            for id in &windows_to_render {
                if let Some(win) = loop_data.state.windows.iter().find(|w| w.id == *id) {
                    if win.cache_width > 0 && win.cache_height > 0 {
                        let is_focused = focused_id == Some(*id);
                        let render_width = (win.cache_width as i32).min(win.geometry.width);
                        let render_height = (win.cache_height as i32).min(win.geometry.height - TITLE_BAR_HEIGHT);
                        
                        if render_width <= 0 || render_height <= 0 {
                            continue;
                        }
                        
                        loop_data.state.canvas.draw_decorations(
                            win.geometry.x, win.geometry.y, 
                            render_width, render_height,
                            TITLE_BAR_HEIGHT, is_focused
                        );
                        
                        let content_y = win.geometry.y + TITLE_BAR_HEIGHT;
                        loop_data.state.canvas.blit_fast(
                            &win.pixel_cache, 
                            render_width as usize, 
                            render_height as usize, 
                            win.cache_stride, 
                            win.geometry.x, 
                            content_y
                        );
                    }
                }
            }
            
            for id in &windows_to_render {
                if let Some(win) = loop_data.state.windows.iter_mut().find(|w| w.id == *id) {
                    win.needs_redraw = false;
                    if let Some(ref buffer) = win.buffer {
                        buffer.release();
                    }
                }
            }
            
            if loop_data.state.cursor_visible {
                loop_data.state.canvas.draw_cursor(loop_data.state.cursor_x, loop_data.state.cursor_y);
            }
        }
        
        loop_data.state.damage_tracker.clear();
    }
    
    if has_pending_screencopy {
        loop_data.state.process_screencopy_frames(true);
    }
    
    let mut buffer = surface.buffer_mut().expect("Failed to get buffer");
    buffer.copy_from_slice(loop_data.state.canvas.as_slice());
    buffer.present().ok();
    
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u32;
        
    for callback in loop_data.state.frame_callbacks.drain(..) {
        callback.done(time);
    }
    
    loop_data.display.flush_clients().ok();
}

fn process_input_frame(data: &mut StandaloneLoopData) {
    let handler = match data.input_handler.as_mut() {
        Some(h) => h,
        None => return,
    };
    
    handler.dispatch().ok();
    let frame = handler.poll_frame();
    
    if !frame.has_events() {
        return;
    }
    
    if frame.exit_compositor {
        session::request_shutdown();
        return;
    }
    
    if frame.launch_terminal {
        let xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| "/tmp".to_string());
        
        match std::process::Command::new("foot")
            .env("WAYLAND_DISPLAY", &data.socket_name)
            .env("XDG_RUNTIME_DIR", &xdg_runtime_dir)
            .stderr(std::process::Stdio::null())
            .spawn() {
            Ok(child) => {
                session::register_child(child.id());
            }
            Err(e) => {
                log::error!("Failed to launch foot: {}", e);
            }
        }
    }
    
    if frame.focus_next {
        data.state.focus_next();
        data.display.flush_clients().ok();
    }
    
    if frame.focus_prev {
        data.state.focus_prev();
        data.display.flush_clients().ok();
    }
    
    if frame.pointer.has_motion {
        let (screen_w, screen_h) = data.state.screen_size();
        
        if let (Some(x), Some(y)) = (frame.pointer.absolute_x, frame.pointer.absolute_y) {
            data.state.handle_pointer_motion(x, y);
        } else if frame.pointer.accumulated_dx != 0.0 || frame.pointer.accumulated_dy != 0.0 {
            let new_x = (data.state.pointer_x + frame.pointer.accumulated_dx)
                .clamp(0.0, screen_w as f64 - 1.0);
            let new_y = (data.state.pointer_y + frame.pointer.accumulated_dy)
                .clamp(0.0, screen_h as f64 - 1.0);
            data.state.handle_pointer_motion(new_x, new_y);
        }
    }
    
    for button in &frame.buttons {
        data.state.handle_pointer_button(button.button, button.pressed);
    }
    
    if frame.pointer.has_scroll {
        data.state.handle_pointer_axis(
            frame.pointer.scroll_horizontal,
            frame.pointer.scroll_vertical,
        );
    }
    
    let focused_keyboards = data.state.get_focused_keyboards();
    if !focused_keyboards.is_empty() {
        for key in &frame.keys {
            let wl_state = match key.state {
                KeyState::Pressed => WlKeyState::Pressed,
                KeyState::Released => WlKeyState::Released,
            };
            
            let serial = data.state.next_keyboard_serial();
            for keyboard in &focused_keyboards {
                log::debug!("[input] Sending key {} to keyboard {:?} client={:?}", 
                    key.keycode, keyboard.id(), keyboard.client().map(|c| c.id()));
                keyboard.key(serial, 0, key.keycode, wl_state);
                keyboard.modifiers(serial, key.mods_depressed, key.mods_latched, key.mods_locked, key.group);
            }
        }
    }
    
    data.display.flush_clients().ok();
}

fn render_standalone(state: &mut State, display: &mut Display<State>, drm_info: Option<&mut DrmInfo>) {
    use std::time::Instant;
    
    if state.needs_relayout {
        state.needs_relayout = false;
        state.relayout_windows();
        display.flush_clients().ok();
    }
    
    let has_pending_screencopy = !state.screencopy_frames.is_empty();
    let has_frame_callbacks = !state.frame_callbacks.is_empty();
    let has_damage = state.damage_tracker.has_damage();
    let cursor_only = state.damage_tracker.is_cursor_only() && !has_pending_screencopy;
    
    if !has_damage && !has_pending_screencopy && !has_frame_callbacks {
        return;
    }
    
    let needs_render = has_damage || has_pending_screencopy;
    
    if needs_render {
        if cursor_only {
            state.canvas.restore_cursor();
            if state.cursor_visible {
                state.canvas.draw_cursor(state.cursor_x, state.cursor_y);
            }
        } else {
            let t0 = Instant::now();
            state.canvas.restore_cursor();
            
            let focused_id = state.focused_window;
            
            let total_windows = state.windows.len();
            let windows_to_render: Vec<_> = state.windows.iter()
                .filter(|w| w.mapped && w.buffer.is_some())
                .map(|w| w.id)
                .collect();
            
            if windows_to_render.len() != total_windows && total_windows > 0 {
                log::warn!("[render] Only rendering {} of {} windows", windows_to_render.len(), total_windows);
                for w in &state.windows {
                    log::warn!(
                        "[render] Window {} status: mapped={} has_buffer={} cache={}x{} geom={:?}",
                        w.id, w.mapped, w.buffer.is_some(), w.cache_width, w.cache_height, w.geometry
                    );
                }
            }
            
            let t1 = Instant::now();
            for id in &windows_to_render {
                state.update_window_pixel_cache(*id);
            }
            let cache_time = t1.elapsed().as_micros();

            let t2 = Instant::now();
            state.canvas.clear_with_pattern();
            let clear_time = t2.elapsed().as_micros();
            
            let t3 = Instant::now();
            for id in &windows_to_render {
                if let Some(win) = state.windows.iter().find(|w| w.id == *id) {
                    if win.cache_width > 0 && win.cache_height > 0 {
                        let is_focused = focused_id == Some(*id);
                        let render_width = (win.cache_width as i32).min(win.geometry.width);
                        let render_height = (win.cache_height as i32).min(win.geometry.height - TITLE_BAR_HEIGHT);
                        
                        if render_width <= 0 || render_height <= 0 {
                            continue;
                        }
                        
                        log::debug!(
                            "[blit] Window {} at ({},{}) size {}x{} cache_ptr={:p} cache_len={} stride={}",
                            win.id, win.geometry.x, win.geometry.y + TITLE_BAR_HEIGHT,
                            render_width, render_height,
                            win.pixel_cache.as_ptr(), win.pixel_cache.len(), win.cache_stride
                        );
                        
                        state.canvas.draw_decorations(
                            win.geometry.x, win.geometry.y,
                            render_width, render_height,
                            TITLE_BAR_HEIGHT, is_focused
                        );
                        
                        let content_y = win.geometry.y + TITLE_BAR_HEIGHT;
                        state.canvas.blit_fast(
                            &win.pixel_cache,
                            render_width as usize,
                            render_height as usize,
                            win.cache_stride,
                            win.geometry.x,
                            content_y
                        );
                    }
                }
            }
            let blit_time = t3.elapsed().as_micros();
            
            for id in &windows_to_render {
                if let Some(win) = state.windows.iter_mut().find(|w| w.id == *id) {
                    win.needs_redraw = false;
                    if let Some(ref buffer) = win.buffer {
                        buffer.release();
                    }
                }
            }
            
            if state.cursor_visible {
                state.canvas.draw_cursor(state.cursor_x, state.cursor_y);
            }
            
            let total = t0.elapsed().as_micros();
            if total > 5000 {
                log::debug!(
                    "[render] total={}us cache={}us clear={}us blit={}us wins={}",
                    total, cache_time, clear_time, blit_time, windows_to_render.len()
                );
            }
        }
        
        if has_damage {
            state.damage_tracker.clear();
        }
    }
    
    if has_pending_screencopy {
        state.process_screencopy_frames(true);
    }
    
    if needs_render {
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
    }
    
    if has_damage || has_frame_callbacks {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u32;
        
        for callback in state.frame_callbacks.drain(..) {
            callback.done(time);
        }
        
        display.flush_clients().ok();
    }
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
    input_pending: bool,
    frame_profiler: FrameProfiler,
}

struct FrameProfiler {
    frame_count: u64,
    last_log_time: std::time::Instant,
    input_time_us: u64,
    render_time_us: u64,
    total_time_us: u64,
    slow_frames: u32,
}

impl FrameProfiler {
    fn new() -> Self {
        Self {
            frame_count: 0,
            last_log_time: std::time::Instant::now(),
            input_time_us: 0,
            render_time_us: 0,
            total_time_us: 0,
            slow_frames: 0,
        }
    }
    
    fn record_frame(&mut self, input_us: u64, render_us: u64, total_us: u64, state: &State) {
        self.frame_count += 1;
        self.input_time_us += input_us;
        self.render_time_us += render_us;
        self.total_time_us += total_us;
        
        if total_us > 16666 {
            self.slow_frames += 1;
        }
        
        if self.last_log_time.elapsed().as_secs() >= 5 {
            let frames = self.frame_count.max(1);
            log::info!(
                "[perf] frames={} slow={} avg_input={}us avg_render={}us avg_total={}us",
                self.frame_count,
                self.slow_frames,
                self.input_time_us / frames,
                self.render_time_us / frames,
                self.total_time_us / frames
            );
            
            Self::log_memory_stats(state);
            
            self.frame_count = 0;
            self.input_time_us = 0;
            self.render_time_us = 0;
            self.total_time_us = 0;
            self.slow_frames = 0;
            self.last_log_time = std::time::Instant::now();
        }
    }
    
    fn log_memory_stats(state: &State) {
        let canvas_bytes = state.canvas.pixels.len() * 4;
        let window_cache_bytes: usize = state.windows.iter()
            .map(|w| w.pixel_cache.len() * 4)
            .sum();
        let total_mb = (canvas_bytes + window_cache_bytes) as f64 / (1024.0 * 1024.0);
        
        log::debug!(
            "[mem] canvas={}KB window_cache={}KB total={:.2}MB windows={}",
            canvas_bytes / 1024,
            window_cache_bytes / 1024,
            total_mb,
            state.windows.len()
        );
    }
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
