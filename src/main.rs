mod state;
mod protocols;
mod input;
mod logging;
mod session;
mod config;
mod renderer;

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
use wayland_protocols_wlr::output_management::v1::server::zwlr_output_manager_v1::ZwlrOutputManagerV1;
use wayland_protocols::wp::linux_dmabuf::zv1::server::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1;
use wayland_protocols::xdg::decoration::zv1::server::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1;
use std::sync::Arc;
use state::State;
use protocols::dmabuf::DmaBufGlobal;
use protocols::xdg_decoration::XdgDecorationGlobal;
use protocols::output_management::OutputManagerGlobal;

fn main() {
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("Error: KTC must not be run as root");
        eprintln!("Add your user to the 'video' and 'input' groups instead:");
        eprintln!("  sudo usermod -aG video,input $USER");
        eprintln!("Then log out and back in.");
        std::process::exit(1);
    }
    
    logging::FileLogger::init().expect("Failed to initialize logging");
    
    let config = Config::load();

    
    log::info!("Starting KTC compositor");
    run(config);
}

fn setup_wayland(has_gpu: bool) -> (Display<State>, ListeningSocket) {
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
    dh.create_global::<State, ZwlrOutputManagerV1, _>(4, OutputManagerGlobal);
    dh.create_global::<State, ZxdgDecorationManagerV1, _>(1, XdgDecorationGlobal);
    
    if has_gpu {
        dh.create_global::<State, ZwpLinuxDmabufV1, _>(4, DmaBufGlobal);
        log::info!("DMA-BUF protocol enabled (GPU acceleration available)");
    }

    let socket = ListeningSocket::bind_auto("wayland", 0..32)
        .expect("Failed to create socket");
    
    log::info!("Listening on: {}", socket.socket_name().unwrap().to_string_lossy());
    
    (display, socket)
}

fn run(config: Config) {
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
    
    let drm_device = if let Some(path) = config.display.drm_device_path() {
        log::info!("Using configured DRM device: {}", path);
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
    } else {
        log::info!("Auto-detecting DRM device");
        OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/dri/card0")
            .or_else(|_| OpenOptions::new().read(true).write(true).open("/dev/dri/card1"))
    };
    
    let preferred_mode = config.display.parse_mode();
    let vsync_enabled = config.display.vsync;
    let gpu_enabled = config.display.gpu;

    let (gpu_renderer, drm_info) = match drm_device {
        Ok(device) => {
            log::info!("Opened DRM device");
            
            if gpu_enabled {
                match renderer::GpuRenderer::new_with_config(
                    device.try_clone().unwrap(),
                    preferred_mode,
                    vsync_enabled,
                ) {
                    Ok(gpu) => {
                        let (w, h) = gpu.size();
                        log::info!("GPU renderer initialized: {}x{}", w, h);
                        (Some(gpu), None)
                    }
                    Err(e) => {
                        log::warn!("GPU renderer failed: {}, falling back to CPU", e);
                        match setup_drm(&device) {
                            Ok(info) => {
                                log::info!("DRM setup complete: {}x{}", info.width, info.height);
                                (None, Some(info))
                            }
                            Err(e) => {
                                log::error!("Failed to setup DRM: {}", e);
                                log::warn!("Running in headless mode");
                                (None, None)
                            }
                        }
                    }
                }
            } else {
                log::info!("GPU rendering disabled by config");
                match setup_drm(&device) {
                    Ok(info) => {
                        log::info!("DRM setup complete: {}x{}", info.width, info.height);
                        (None, Some(info))
                    }
                    Err(e) => {
                        log::error!("Failed to setup DRM: {}", e);
                        log::warn!("Running in headless mode");
                        (None, None)
                    }
                }
            }
        }
        Err(e) => {
            log::error!("Failed to open DRM device: {}", e);
            log::warn!("Running in headless mode (no display output)");
            log::info!("Make sure you're in the 'video' group: sudo usermod -aG video $USER");
            (None, None)
        }
    };

    let has_gpu = gpu_renderer.is_some();
    let (mut display, socket) = setup_wayland(has_gpu);
    
    let socket_name = socket.socket_name()
        .expect("Failed to get socket name")
        .to_string_lossy()
        .to_string();

    let keybinds: std::collections::HashMap<String, crate::config::Keybind> = config
        .keybinds
        .get_all_bindings()
        .into_iter()
        .collect();
    


    let input_handler = match InputHandler::new(keybinds) {
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

    if let Some(ref gpu) = gpu_renderer {
        let drm_fd = gpu.drm_fd().try_clone_to_owned()
            .expect("Failed to clone DRM fd");
        
        calloop_loop
            .handle()
            .insert_source(
                calloop::generic::Generic::new(
                    drm_fd,
                    calloop::Interest::READ,
                    calloop::Mode::Level,
                ),
                |_, _, data| {
                    data.vsync_pending = true;
                    Ok(calloop::PostAction::Continue)
                },
            )
            .expect("Failed to insert DRM source");
    }

    let _timer = calloop_loop.handle()
        .insert_source(
            calloop::timer::Timer::immediate(),
            |_deadline, _: &mut (), data| {
                let frame_start = std::time::Instant::now();
                
                let input_start = std::time::Instant::now();
                if data.input_pending {
                    data.input_pending = false;
                    process_input(data);
                }
                let input_time = input_start.elapsed().as_micros() as u64;
                
                if data.vsync_pending {
                    data.vsync_pending = false;
                    if let Some(ref mut gpu) = data.state.gpu_renderer {
                        if gpu.handle_drm_event() {
                            let time = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u32;
                            
                            for callback in data.state.frame_callbacks.drain(..) {
                                callback.done(time);
                            }
                            data.display.flush_clients().ok();
                        }
                    }
                }
                
                let profiler_stats = data.frame_profiler.get_stats(&data.state);
                let show_profiler = data.state.config.debug.profiler;
                
                let can_render = data.state.gpu_renderer.as_ref()
                    .map(|gpu| !gpu.is_flip_pending())
                    .unwrap_or(true);
                
                let render_start = std::time::Instant::now();
                if can_render {
                    render(&mut data.state, &mut data.display, data.drm_info.as_mut(), 
                           if show_profiler { Some(&profiler_stats) } else { None });
                }
                let render_time = render_start.elapsed().as_micros() as u64;
                
                let total_time = frame_start.elapsed().as_micros() as u64;
                data.frame_profiler.record_frame(input_time, render_time, total_time, &data.state);
                
                let timeout = if data.state.gpu_renderer.is_some() {
                    std::time::Duration::from_millis(1)
                } else {
                    std::time::Duration::from_millis(16)
                };
                calloop::timer::TimeoutAction::ToDuration(timeout)
            },
        )
        .expect("Failed to insert timer");

    let mut loop_data = LoopData {
        display,
        state: State::new(config),
        drm_info,
        input_handler,
        socket_name,
        input_pending: false,
        vsync_pending: false,
        frame_profiler: FrameProfiler::new(),
    };
    
    loop_data.state.gpu_renderer = gpu_renderer;
    
    if let Some(ref gpu) = loop_data.state.gpu_renderer {
        let (w, h) = gpu.size();
        use state::OutputConfig;
        let output_id = loop_data.state.add_output("GPU".to_string(), w as i32, h as i32);
        loop_data.state.configure_output(output_id, OutputConfig {
            make: Some("GPU".to_string()),
            model: Some("OpenGL".to_string()),
            ..Default::default()
        });
        log::info!("Configured GPU output at {}x{}", w, h);
    } else if let Some(ref drm) = loop_data.drm_info {
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

    log::info!("Compositor running. Press Ctrl+Alt+Q to exit.");
    
    while session::is_running() {
        calloop_loop.dispatch(Some(std::time::Duration::from_millis(16)), &mut loop_data)
            .expect("Event loop error");
    }
    
    log::info!("Main loop exited, cleaning up...");
}

fn process_input(data: &mut LoopData) {
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
    
    if let Some(ref cmd) = frame.exec_command {
        let xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| "/tmp".to_string());
        
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if let Some((program, args)) = parts.split_first() {
            match std::process::Command::new(program)
                .args(args)
                .env("WAYLAND_DISPLAY", &data.socket_name)
                .env("XDG_RUNTIME_DIR", &xdg_runtime_dir)
                .stderr(std::process::Stdio::null())
                .spawn() {
                Ok(child) => {
                    session::register_child(child.id());
                    log::info!("Launched: {}", cmd);
                }
                Err(e) => {
                    log::error!("Failed to launch '{}': {}", cmd, e);
                }
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
    
    if frame.close_window {
        if let Some(focused_id) = data.state.focused_window {
            data.state.close_window(focused_id);
            data.display.flush_clients().ok();
        }
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
                keyboard.key(serial, 0, key.keycode, wl_state);
                keyboard.modifiers(serial, key.mods_depressed, key.mods_latched, key.mods_locked, key.group);
            }
        }
    }
    
    data.display.flush_clients().ok();
}

fn render(state: &mut State, display: &mut Display<State>, drm_info: Option<&mut DrmInfo>, profiler_stats: Option<&renderer::ProfilerStats>) {
    if state.gpu_renderer.is_some() {
        render_gpu(state, display, profiler_stats);
        return;
    }
    
    render_cpu(state, display, drm_info);
}

fn render_gpu(state: &mut State, display: &mut Display<State>, profiler_stats: Option<&renderer::ProfilerStats>) {
    if state.needs_relayout {
        state.needs_relayout = false;
        state.relayout_windows();
        display.flush_clients().ok();
    }
    
    let has_pending_screencopy = !state.screencopy_frames.is_empty();
    let has_frame_callbacks = !state.frame_callbacks.is_empty();
    let has_damage = state.damage_tracker.has_damage();
    let has_profiler = profiler_stats.is_some();
    
    if !has_damage && !has_pending_screencopy && !has_frame_callbacks && !has_profiler {
        return;
    }
    
    let needs_render = has_damage || has_pending_screencopy || has_profiler;
    
    if needs_render {
        let bg_dark = state.config.background_dark();
        let title_focused = state.config.title_focused();
        let title_unfocused = state.config.title_unfocused();
        let title_bar_height = state.config.title_bar_height();
        let focused_id = state.focused_window;
        let windows_needing_update: Vec<_> = state.windows.iter()
            .filter(|w| w.mapped && w.buffer.is_some() && w.needs_redraw)
            .map(|w| w.id)
            .collect();
        
        for id in &windows_needing_update {
            state.update_window_pixel_cache(*id);
        }
        
        let window_render_info: Vec<_> = state.windows.iter()
            .filter(|w| w.mapped && w.buffer.is_some())
            .map(|w| {
                let buffer_id = w.buffer.as_ref().map(|b| b.id());
                let is_shm = buffer_id.as_ref().map(|id| state.buffers.contains_key(id)).unwrap_or(false);
                (w.id, w.geometry, w.cache_width, w.cache_height, w.cache_stride, is_shm, buffer_id)
            })
            .collect();
        
        let gpu = state.gpu_renderer.as_mut().unwrap();
        
        gpu.begin_frame();
        
        let (width, height) = gpu.size();
        let bg_color = [
            ((bg_dark >> 16) & 0xFF) as f32 / 255.0,
            ((bg_dark >> 8) & 0xFF) as f32 / 255.0,
            (bg_dark & 0xFF) as f32 / 255.0,
            1.0,
        ];
        gpu.draw_rect(0, 0, width as i32, height as i32, bg_color);
        
        for (id, geom, cache_w, cache_h, cache_stride, is_shm, buffer_id) in &window_render_info {
            let is_focused = focused_id == Some(*id);
            let title_color = if is_focused { title_focused } else { title_unfocused };
            let title_rgba = [
                ((title_color >> 16) & 0xFF) as f32 / 255.0,
                ((title_color >> 8) & 0xFF) as f32 / 255.0,
                (title_color & 0xFF) as f32 / 255.0,
                1.0,
            ];
            
            let gpu = state.gpu_renderer.as_mut().unwrap();
            gpu.draw_rect(geom.x, geom.y, geom.width, title_bar_height, title_rgba);
            
            let content_y = geom.y + title_bar_height;
            
            if *is_shm {
                let win = match state.windows.iter().find(|w| w.id == *id) {
                    Some(w) if !w.pixel_cache.is_empty() && *cache_w > 0 && *cache_h > 0 => w,
                    _ => continue,
                };
                
                let data: &[u8] = unsafe {
                    std::slice::from_raw_parts(
                        win.pixel_cache.as_ptr() as *const u8,
                        win.pixel_cache.len() * 4,
                    )
                };
                let gpu = state.gpu_renderer.as_mut().unwrap();
                let texture = gpu.upload_shm_texture(*id, *cache_w as u32, *cache_h as u32, *cache_stride as u32, data);
                
                let gpu = state.gpu_renderer.as_mut().unwrap();
                gpu.draw_texture(
                    texture,
                    geom.x,
                    content_y,
                    *cache_w as i32,
                    *cache_h as i32,
                );
            } else if let Some(buf_id) = buffer_id {
                if let Some(dmabuf_info) = state.dmabuf_buffers.get(buf_id) {
                    let info = dmabuf_info.clone();
                    let buffer_cache_id = buf_id.protocol_id() as u64;
                    let gpu = state.gpu_renderer.as_mut().unwrap();
                    if let Some(texture) = gpu.import_dmabuf_texture(
                        buffer_cache_id,
                        info.fd,
                        info.width as u32,
                        info.height as u32,
                        info.format,
                        info.stride,
                        info.offset,
                        info.modifier,
                    ) {
                        gpu.draw_texture(
                            texture,
                            geom.x,
                            content_y,
                            info.width,
                            info.height,
                        );
                    } else {
                        log::warn!("[render] DMA-BUF texture import failed for window {}", id);
                    }
                } else {
                    log::warn!("[render] No dmabuf_info for buffer {:?}", buf_id);
                }
            }
        }
        
        if let Some(stats) = profiler_stats {
            let gpu = state.gpu_renderer.as_mut().unwrap();
            gpu.draw_profiler(stats);
        }
        
        let gpu = state.gpu_renderer.as_mut().unwrap();
        gpu.end_frame();
        
        for id in &windows_needing_update {
            if let Some(win) = state.windows.iter_mut().find(|w| w.id == *id) {
                win.needs_redraw = false;
                if let Some(ref buffer) = win.buffer {
                    buffer.release();
                }
            }
        }
        
        if has_damage {
            state.damage_tracker.clear();
        }
    }
    
    if has_pending_screencopy {
        state.process_screencopy_frames(true);
    }
    
    if has_frame_callbacks {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u32;
        
        for callback in state.frame_callbacks.drain(..) {
            callback.done(time);
        }
    }
    
    display.flush_clients().ok();
}

fn render_cpu(state: &mut State, display: &mut Display<State>, drm_info: Option<&mut DrmInfo>) {
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
            state.canvas.restore_cursor();
            
            let focused_id = state.focused_window;
            
            let windows_to_render: Vec<_> = state.windows.iter()
                .filter(|w| w.mapped && w.buffer.is_some())
                .map(|w| w.id)
                .collect();
            
            for id in &windows_to_render {
                state.update_window_pixel_cache(*id);
            }

            state.canvas.clear_with_pattern(
                state.config.background_dark(),
                state.config.background_light()
            );
            
            let title_focused = state.config.title_focused();
            let title_unfocused = state.config.title_unfocused();
            let border_focused = state.config.border_focused();
            let border_unfocused = state.config.border_unfocused();
            let title_bar_height = state.config.title_bar_height();
            
            for id in &windows_to_render {
                if let Some(win) = state.windows.iter().find(|w| w.id == *id) {
                    if win.cache_width > 0 && win.cache_height > 0 {
                        let is_focused = focused_id == Some(*id);
                        let render_width = (win.cache_width as i32).min(win.geometry.width);
                        let render_height = (win.cache_height as i32).min(win.geometry.height - title_bar_height);
                        
                        if render_width <= 0 || render_height <= 0 {
                            continue;
                        }
                        
                        state.canvas.draw_decorations(
                            win.geometry.x, win.geometry.y,
                            render_width, render_height,
                            title_bar_height, is_focused,
                            title_focused, title_unfocused, border_focused, border_unfocused
                        );
                        
                        let content_y = win.geometry.y + title_bar_height;
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

struct LoopData {
    display: Display<State>,
    state: State,
    drm_info: Option<DrmInfo>,
    input_handler: Option<input::InputHandler>,
    socket_name: String,
    input_pending: bool,
    vsync_pending: bool,
    frame_profiler: FrameProfiler,
}

struct FrameProfiler {
    frame_count: u64,
    last_log_time: std::time::Instant,
    input_time_us: u64,
    render_time_us: u64,
    total_time_us: u64,
    slow_frames: u32,
    last_fps: f32,
    last_frame_time_ms: f32,
    last_render_us: u64,
    last_input_us: u64,
    fps_update_time: std::time::Instant,
    fps_frame_count: u64,
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
            last_fps: 0.0,
            last_frame_time_ms: 0.0,
            last_render_us: 0,
            last_input_us: 0,
            fps_update_time: std::time::Instant::now(),
            fps_frame_count: 0,
        }
    }
    
    fn record_frame(&mut self, input_us: u64, render_us: u64, total_us: u64, state: &State) {
        self.frame_count += 1;
        self.fps_frame_count += 1;
        self.input_time_us += input_us;
        self.render_time_us += render_us;
        self.total_time_us += total_us;
        self.last_render_us = render_us;
        self.last_input_us = input_us;
        self.last_frame_time_ms = total_us as f32 / 1000.0;
        
        if total_us > 16666 {
            self.slow_frames += 1;
        }
        
        let fps_elapsed = self.fps_update_time.elapsed();
        if fps_elapsed.as_millis() >= 500 {
            self.last_fps = self.fps_frame_count as f32 / fps_elapsed.as_secs_f32();
            self.fps_frame_count = 0;
            self.fps_update_time = std::time::Instant::now();
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
    
    fn get_stats(&self, state: &State) -> renderer::ProfilerStats {
        let canvas_bytes = state.canvas.pixels.len() * 4;
        let window_cache_bytes: usize = state.windows.iter()
            .map(|w| w.pixel_cache.len() * 4)
            .sum();
        let memory_mb = (canvas_bytes + window_cache_bytes) as f32 / (1024.0 * 1024.0);
        
        let texture_count = state.gpu_renderer.as_ref()
            .map(|r| r.texture_count())
            .unwrap_or(0);
        
        renderer::ProfilerStats {
            fps: self.last_fps,
            frame_time_ms: self.last_frame_time_ms,
            render_time_us: self.last_render_us,
            input_time_us: self.last_input_us,
            memory_mb,
            window_count: state.windows.len(),
            texture_count,
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
    _fb_id: u32,
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
        _fb_id: fb_handle.into(),
        _crtc: crtc_handle,
        physical_width: phys_width,
        physical_height: phys_height,
        refresh,
        name: connector_name,
    })
}
