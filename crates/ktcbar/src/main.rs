use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{wl_registry, wl_compositor, wl_shm, wl_surface, wl_buffer, wl_output, wl_shm_pool},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};
use std::os::unix::io::AsFd;

const BAR_HEIGHT: u32 = 32;

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
}

impl AppState {
    fn new() -> Self {
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

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.width == 0 {
            return;
        }

        let Some(shm) = &self.shm else { return };
        let Some(surface) = &self.surface else { return };

        let stride = self.width * 4;
        let size = (stride * self.height) as usize;

        let file = create_shm_file(size);
        let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            self.width as i32,
            self.height as i32,
            stride as i32,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );

        unsafe {
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                std::os::unix::io::AsRawFd::as_raw_fd(&file),
                0,
            );
            if ptr != libc::MAP_FAILED {
                let pixels = std::slice::from_raw_parts_mut(ptr as *mut u32, size / 4);
                let bg_color = 0xFF1A1A2E;
                pixels.fill(bg_color);
                libc::munmap(ptr, size);
            }
        }

        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, self.width as i32, self.height as i32);
        surface.commit();

        pool.destroy();
    }
}

fn create_shm_file(size: usize) -> std::fs::File {
    use std::os::unix::io::FromRawFd;
    
    let name = format!("/ktcbar-{}", std::process::id());
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
        _proxy: &wl_buffer::WlBuffer,
        _event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
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

    event_queue.roundtrip(&mut state).expect("Roundtrip failed");

    while state.running {
        event_queue.blocking_dispatch(&mut state).expect("Dispatch failed");
    }
}
