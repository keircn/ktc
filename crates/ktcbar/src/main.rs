use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{wl_registry, wl_compositor, wl_shm, wl_surface, wl_buffer, wl_output, wl_shm_pool, wl_callback},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::AsFd;
use std::os::unix::net::UnixStream;
use chrono::Local;
use ktc_common::{Font, IpcCommand, IpcEvent, WorkspaceInfo, ipc_socket_path};

const BAR_HEIGHT: u32 = 24;
const BG_COLOR: u32 = 0xFF1A1A2E;
const TEXT_COLOR: u32 = 0xFFE0E0E0;
const ACTIVE_WS_COLOR: u32 = 0xFF4A9EFF;
const INACTIVE_WS_COLOR: u32 = 0xFF505050;
const WS_HAS_WINDOWS_COLOR: u32 = 0xFF808080;

struct IpcClient {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl IpcClient {
    fn connect() -> Option<Self> {
        let path = ipc_socket_path();
        let stream = UnixStream::connect(&path).ok()?;
        stream.set_nonblocking(true).ok()?;
        let reader = BufReader::new(stream.try_clone().ok()?);
        Some(Self { stream, reader })
    }
    
    fn send_command(&mut self, cmd: &IpcCommand) {
        if let Ok(json) = serde_json::to_string(cmd) {
            let _ = writeln!(self.stream, "{}", json);
        }
    }
    
    fn poll_events(&mut self) -> Vec<IpcEvent> {
        let mut events = Vec::new();
        let mut line = String::new();
        
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if let Ok(event) = serde_json::from_str::<IpcEvent>(line.trim()) {
                        events.push(event);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        
        events
    }
}

struct AppState {
    compositor: Option<wl_compositor::WlCompositor>,
    layer_shell: Option<ZwlrLayerShellV1>,
    shm: Option<wl_shm::WlShm>,
    output: Option<wl_output::WlOutput>,
    surface: Option<wl_surface::WlSurface>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,
    configured: bool,
    width: u32,
    height: u32,
    running: bool,
    font: Font,
    workspaces: Vec<WorkspaceInfo>,
    active_workspace: usize,
    focused_title: Option<String>,
    needs_redraw: bool,
    ipc_client: Option<IpcClient>,
}

impl AppState {
    fn new() -> Self {
        let ipc_client = IpcClient::connect();
        let workspaces = (1..=4).map(WorkspaceInfo::new).collect();
        
        Self {
            compositor: None,
            layer_shell: None,
            shm: None,
            output: None,
            surface: None,
            layer_surface: None,
            configured: false,
            width: 0,
            height: BAR_HEIGHT,
            running: true,
            font: Font::new(2),
            workspaces,
            active_workspace: 1,
            focused_title: None,
            needs_redraw: false,
            ipc_client,
        }
    }
    
    fn request_state(&mut self) {
        if let Some(ref mut ipc) = self.ipc_client {
            ipc.send_command(&IpcCommand::GetState);
        }
    }
    
    fn poll_ipc(&mut self) {
        let events = if let Some(ref mut ipc) = self.ipc_client {
            ipc.poll_events()
        } else {
            return;
        };
        
        for event in events {
            match event {
                IpcEvent::State { workspaces, active_workspace, focused_window } => {
                    self.workspaces = workspaces;
                    self.active_workspace = active_workspace;
                    self.focused_title = focused_window;
                    self.needs_redraw = true;
                }
                IpcEvent::WorkspaceChanged { workspaces, active_workspace } => {
                    self.workspaces = workspaces;
                    self.active_workspace = active_workspace;
                    self.needs_redraw = true;
                }
                IpcEvent::FocusChanged { window_title } => {
                    self.focused_title = window_title;
                    self.needs_redraw = true;
                }
                IpcEvent::TitleChanged { window_title } => {
                    self.focused_title = Some(window_title);
                    self.needs_redraw = true;
                }
            }
        }
    }

    fn create_layer_surface(&mut self, qh: &QueueHandle<Self>) {
        let Some(compositor) = &self.compositor else { return };
        let Some(layer_shell) = &self.layer_shell else { return };

        let surface = compositor.create_surface(qh, ());
        let layer_surface = layer_shell.get_layer_surface(
            &surface,
            self.output.as_ref(),
            zwlr_layer_shell_v1::Layer::Top,
            "ktcbar".to_string(),
            qh,
            (),
        );

        layer_surface.set_size(0, BAR_HEIGHT);
        layer_surface.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right,
        );
        layer_surface.set_exclusive_zone(BAR_HEIGHT as i32);
        surface.commit();

        self.surface = Some(surface);
        self.layer_surface = Some(layer_surface);
    }

    fn request_frame(&self, qh: &QueueHandle<Self>) {
        if let Some(surface) = &self.surface {
            surface.frame(qh, ());
        }
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.width == 0 {
            return;
        }

        let Some(shm) = &self.shm else { return };
        let Some(surface) = &self.surface else { return };

        let stride = self.width;
        let size = (stride * self.height) as usize;
        let byte_size = size * 4;

        let file = create_shm_file(byte_size);
        let pool = shm.create_pool(file.as_fd(), byte_size as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            self.width as i32,
            self.height as i32,
            (stride * 4) as i32,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );

        unsafe {
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                byte_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                std::os::unix::io::AsRawFd::as_raw_fd(&file),
                0,
            );
            if ptr != libc::MAP_FAILED {
                let pixels = std::slice::from_raw_parts_mut(ptr as *mut u32, size);
                self.render(pixels, stride as usize);
                libc::munmap(ptr, byte_size);
            }
        }

        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, self.width as i32, self.height as i32);
        surface.commit();

        pool.destroy();
        self.needs_redraw = false;
    }

    fn render(&self, pixels: &mut [u32], stride: usize) {
        pixels.fill(BG_COLOR);

        let padding = 8;
        let text_y = (self.height as usize - self.font.char_height()) / 2;

        self.draw_workspaces(pixels, stride, padding, text_y);
        self.draw_title(pixels, stride, text_y);
        self.draw_clock(pixels, stride, self.width as usize - padding, text_y);
    }

    fn draw_workspaces(&self, pixels: &mut [u32], stride: usize, x: usize, y: usize) {
        let mut current_x = x;
        let ws_width = self.font.char_width() + 8;

        for ws in &self.workspaces {
            let is_active = ws.id == self.active_workspace;
            let has_windows = ws.window_count > 0;
            
            let color = if is_active {
                ACTIVE_WS_COLOR
            } else if has_windows {
                WS_HAS_WINDOWS_COLOR
            } else {
                INACTIVE_WS_COLOR
            };

            if is_active {
                fill_rect(pixels, stride, self.height as usize, current_x - 2, y - 2, ws_width, self.font.char_height() + 4, 0xFF2D3A4A);
            }

            let num = char::from_digit(ws.id as u32, 10).unwrap_or('?');
            self.font.draw_char(pixels, stride, current_x + 2, y, num, color);
            current_x += ws_width + 4;
        }
    }
    
    fn draw_title(&self, pixels: &mut [u32], stride: usize, y: usize) {
        if let Some(ref title) = self.focused_title {
            let max_title_len = 40;
            let title = if title.len() > max_title_len {
                format!("{}...", &title[..max_title_len - 3])
            } else {
                title.clone()
            };
            
            let title_width = self.font.text_width(&title);
            let center_x = (self.width as usize / 2).saturating_sub(title_width / 2);
            self.font.draw_text(pixels, stride, center_x, y, &title, TEXT_COLOR);
        }
    }

    fn draw_clock(&self, pixels: &mut [u32], stride: usize, right_x: usize, y: usize) {
        let now = Local::now();
        let time_str = now.format("%H:%M").to_string();
        self.font.draw_text_right(pixels, stride, right_x, y, &time_str, TEXT_COLOR);
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_rect(pixels: &mut [u32], stride: usize, height: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    for dy in 0..h {
        for dx in 0..w {
            let px = x + dx;
            let py = y + dy;
            if py < height && px < stride {
                let idx = py * stride + px;
                if idx < pixels.len() {
                    pixels[idx] = color;
                }
            }
        }
    }
}

fn create_shm_file(size: usize) -> std::fs::File {
    use std::os::unix::io::FromRawFd;

    let name = format!("/ktcbar-{}-{}", std::process::id(), std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0));
    let fd = unsafe {
        libc::shm_open(
            std::ffi::CString::new(name.clone()).unwrap().as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
    };

    if fd < 0 {
        panic!("Failed to create shm file");
    }

    unsafe {
        libc::shm_unlink(std::ffi::CString::new(name).unwrap().as_ptr());
        libc::ftruncate(fd, size as i64);
        std::fs::File::from_raw_fd(fd)
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind(name, version.min(1), qh, ()));
                }
                "wl_output" => {
                    if state.output.is_none() {
                        state.output = Some(registry.bind(name, version.min(4), qh, ()));
                    }
                }
                "zwlr_layer_shell_v1" => {
                    state.layer_shell = Some(registry.bind(name, version.min(4), qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_compositor::WlCompositor,
        _event: wl_compositor::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_shm::WlShm, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm::WlShm,
        _event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm_pool::WlShmPool,
        _event: wl_shm_pool::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_output::WlOutput, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_output::WlOutput,
        _event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_surface::WlSurface, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_surface::WlSurface,
        _event: wl_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_buffer::WlBuffer, ()> for AppState {
    fn event(
        _state: &mut Self,
        buffer: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            buffer.destroy();
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            if state.needs_redraw {
                state.draw(qh);
            }
            state.request_frame(qh);
        }
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrLayerShellV1,
        _event: zwlr_layer_shell_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for AppState {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                layer_surface.ack_configure(serial);
                state.width = width;
                state.height = if height > 0 { height } else { BAR_HEIGHT };
                state.configured = true;
                state.draw(qh);
                state.request_frame(qh);
            }
            zwlr_layer_surface_v1::Event::Closed => {
                state.running = false;
            }
            _ => {}
        }
    }
}

fn main() {
    let conn = Connection::connect_to_env().expect("Failed to connect to Wayland");
    let display = conn.display();

    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let _registry = display.get_registry(&qh, ());

    let mut state = AppState::new();

    event_queue.roundtrip(&mut state).expect("Roundtrip failed");

    state.create_layer_surface(&qh);
    
    state.request_state();

    event_queue.roundtrip(&mut state).expect("Roundtrip failed");

    use std::time::{Duration, Instant};
    let mut last_clock_update = Instant::now();
    let clock_interval = Duration::from_secs(1);

    while state.running {
        state.poll_ipc();
        
        if last_clock_update.elapsed() >= clock_interval {
            state.needs_redraw = true;
            last_clock_update = Instant::now();
        }

        if let Err(e) = event_queue.dispatch_pending(&mut state) {
            eprintln!("Dispatch error: {}", e);
            break;
        }

        if let Err(e) = event_queue.flush() {
            eprintln!("Flush error: {}", e);
            break;
        }

        std::thread::sleep(Duration::from_millis(16));
    }
}
