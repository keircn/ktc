use input::event::{Event, EventTrait};
use input::event::keyboard::KeyboardEventTrait;
use input::event::pointer::PointerScrollEvent;
pub use input::event::keyboard::KeyState;
use input::{Libinput, LibinputInterface};
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Instant;
use xkbcommon::xkb;
use xkbcommon::xkb::keysyms::{KEY_q, KEY_t, KEY_Tab, KEY_j, KEY_k};

struct Interface;

impl LibinputInterface for Interface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        match OpenOptions::new()
            .custom_flags(flags)
            .read(flags & libc::O_RDWR != 0)
            .write(flags & libc::O_RDWR != 0)
            .open(path)
        {
            Ok(file) => {
                log::debug!("[input] Opened device: {}", path.display());
                Ok(file.into())
            }
            Err(err) => {
                log::warn!("[input] Failed to open {}: {}", path.display(), err);
                Err(err.raw_os_error().unwrap_or(-1))
            }
        }
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        drop(File::from(fd));
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InputStats {
    pub motion_events: u32,
    pub button_events: u32,
    pub key_events: u32,
    pub process_time_us: u64,
}

impl InputStats {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PointerState {
    pub accumulated_dx: f64,
    pub accumulated_dy: f64,
    pub absolute_x: Option<f64>,
    pub absolute_y: Option<f64>,
    pub scroll_horizontal: f64,
    pub scroll_vertical: f64,
    pub has_motion: bool,
    pub has_scroll: bool,
}

impl Default for PointerState {
    fn default() -> Self {
        Self {
            accumulated_dx: 0.0,
            accumulated_dy: 0.0,
            absolute_x: None,
            absolute_y: None,
            scroll_horizontal: 0.0,
            scroll_vertical: 0.0,
            has_motion: false,
            has_scroll: false,
        }
    }
}

impl PointerState {
    pub fn reset(&mut self) {
        self.accumulated_dx = 0.0;
        self.accumulated_dy = 0.0;
        self.absolute_x = None;
        self.absolute_y = None;
        self.scroll_horizontal = 0.0;
        self.scroll_vertical = 0.0;
        self.has_motion = false;
        self.has_scroll = false;
    }

    pub fn accumulate_relative(&mut self, dx: f64, dy: f64) {
        self.accumulated_dx += dx;
        self.accumulated_dy += dy;
        self.has_motion = true;
    }

    pub fn set_absolute(&mut self, x: f64, y: f64) {
        self.absolute_x = Some(x);
        self.absolute_y = Some(y);
        self.has_motion = true;
    }

    pub fn accumulate_scroll(&mut self, h: f64, v: f64) {
        self.scroll_horizontal += h;
        self.scroll_vertical += v;
        self.has_scroll = true;
    }
}

#[derive(Clone, Debug)]
pub struct ButtonEvent {
    pub button: u32,
    pub pressed: bool,
}

#[derive(Clone, Debug)]
pub struct KeyEvent {
    pub keycode: u32,
    pub state: KeyState,
    pub mods_depressed: u32,
    pub mods_latched: u32,
    pub mods_locked: u32,
    pub group: u32,
}

#[derive(Clone, Debug, Default)]
pub struct InputFrame {
    pub pointer: PointerState,
    pub buttons: Vec<ButtonEvent>,
    pub keys: Vec<KeyEvent>,
    pub exit_compositor: bool,
    pub launch_terminal: bool,
    pub focus_next: bool,
    pub focus_prev: bool,
}

impl InputFrame {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.pointer.reset();
        self.buttons.clear();
        self.keys.clear();
        self.exit_compositor = false;
        self.launch_terminal = false;
        self.focus_next = false;
        self.focus_prev = false;
    }

    pub fn has_events(&self) -> bool {
        self.pointer.has_motion
            || self.pointer.has_scroll
            || !self.buttons.is_empty()
            || !self.keys.is_empty()
            || self.exit_compositor
            || self.launch_terminal
            || self.focus_next
            || self.focus_prev
    }
}

pub struct InputHandler {
    libinput: Libinput,
    xkb_context: xkb::Context,
    xkb_state: Option<xkb::State>,
    ctrl: bool,
    alt: bool,
    frame: InputFrame,
    stats: InputStats,
    last_stats_log: Instant,
    frame_count: u64,
}

impl InputHandler {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut libinput = Libinput::new_with_udev(Interface);
        libinput.udev_assign_seat("seat0")
            .map_err(|_| "Failed to assign udev seat")?;
        
        libinput.dispatch()?;
        let mut keyboard_count = 0;
        let mut pointer_count = 0;
        for event in &mut libinput {
            if let Event::Device(input::event::DeviceEvent::Added(added)) = event {
                let device = added.device();
                if device.has_capability(input::DeviceCapability::Keyboard) {
                    keyboard_count += 1;
                    log::info!("[input] Keyboard device: {}", device.name());
                }
                if device.has_capability(input::DeviceCapability::Pointer) {
                    pointer_count += 1;
                    log::info!("[input] Pointer device: {}", device.name());
                }
            }
        }
        
        if keyboard_count == 0 {
            log::error!("[input] No keyboard devices found! Check /dev/input permissions or add user to 'input' group");
        }
        if pointer_count == 0 {
            log::warn!("[input] No pointer devices found");
        }
        
        let xkb_context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        
        Ok(InputHandler {
            libinput,
            xkb_context,
            xkb_state: None,
            ctrl: false,
            alt: false,
            frame: InputFrame::new(),
            stats: InputStats::default(),
            last_stats_log: Instant::now(),
            frame_count: 0,
        })
    }
    
    pub fn dispatch(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.libinput.dispatch()?;
        Ok(())
    }

    pub fn poll_frame(&mut self) -> &InputFrame {
        let start = Instant::now();
        self.frame.reset();
        self.stats.reset();
        
        let mut keyboard_events = Vec::new();
        let mut pointer_events = Vec::new();
        let mut has_keyboard_device = false;
        let mut raw_motion_count = 0u32;
        
        for event in &mut self.libinput {
            match event {
                Event::Keyboard(keyboard_event) => {
                    let key = keyboard_event.key();
                    let state = keyboard_event.key_state();
                    keyboard_events.push((key, state));
                }
                Event::Device(device_event) => {
                    use input::event::DeviceEvent;
                    if let DeviceEvent::Added(added) = device_event {
                        let device = added.device();
                        if device.has_capability(input::DeviceCapability::Keyboard) {
                            has_keyboard_device = true;
                        }
                    }
                }
                Event::Pointer(pointer_event) => {
                    if matches!(pointer_event, input::event::PointerEvent::Motion(_)) {
                        raw_motion_count += 1;
                    }
                    pointer_events.push(pointer_event);
                }
                _ => {}
            }
        }
        
        self.stats.motion_events = raw_motion_count;
        self.stats.button_events = pointer_events.iter()
            .filter(|e| matches!(e, input::event::PointerEvent::Button(_)))
            .count() as u32;
        self.stats.key_events = keyboard_events.len() as u32;
        
        for pointer_event in pointer_events {
            self.handle_pointer_event(pointer_event);
        }
        
        if has_keyboard_device {
            self.init_xkb_state();
        }
        
        for (key, state) in keyboard_events {
            self.handle_keyboard_key_batched(key, state);
        }
        
        self.stats.process_time_us = start.elapsed().as_micros() as u64;
        self.frame_count += 1;
        
        if self.last_stats_log.elapsed().as_secs() >= 2 {
            if self.stats.motion_events > 0 || self.stats.button_events > 0 {
                log::debug!(
                    "[input] frame={} motion={} btn={} keys={} time={}us",
                    self.frame_count,
                    self.stats.motion_events,
                    self.stats.button_events,
                    self.stats.key_events,
                    self.stats.process_time_us
                );
            }
            self.last_stats_log = Instant::now();
        }
        
        &self.frame
    }

    fn handle_pointer_event(&mut self, pointer_event: input::event::PointerEvent) {
        use input::event::PointerEvent;
        use input::event::pointer::ButtonState;
        
        match pointer_event {
            PointerEvent::Motion(motion) => {
                self.frame.pointer.accumulate_relative(motion.dx(), motion.dy());
            }
            PointerEvent::MotionAbsolute(abs) => {
                self.frame.pointer.set_absolute(abs.absolute_x(), abs.absolute_y());
            }
            PointerEvent::Button(btn) => {
                self.frame.buttons.push(ButtonEvent {
                    button: btn.button(),
                    pressed: btn.button_state() == ButtonState::Pressed,
                });
            }
            PointerEvent::ScrollWheel(scroll) => {
                let h = scroll.scroll_value_v120(input::event::pointer::Axis::Horizontal) / 120.0 * 15.0;
                let v = scroll.scroll_value_v120(input::event::pointer::Axis::Vertical) / 120.0 * 15.0;
                self.frame.pointer.accumulate_scroll(h, v);
            }
            PointerEvent::ScrollFinger(scroll) => {
                let h = scroll.scroll_value(input::event::pointer::Axis::Horizontal);
                let v = scroll.scroll_value(input::event::pointer::Axis::Vertical);
                self.frame.pointer.accumulate_scroll(h, v);
            }
            PointerEvent::ScrollContinuous(scroll) => {
                let h = scroll.scroll_value(input::event::pointer::Axis::Horizontal);
                let v = scroll.scroll_value(input::event::pointer::Axis::Vertical);
                self.frame.pointer.accumulate_scroll(h, v);
            }
            _ => {}
        }
    }

    fn handle_keyboard_key_batched(&mut self, key: u32, state: input::event::keyboard::KeyState) {
        use input::event::keyboard::KeyState;
        
        if self.xkb_state.is_none() {
            self.init_xkb_state();
        }
        
        if let Some(ref mut xkb_state) = self.xkb_state {
            let keycode = key + 8;
            
            xkb_state.update_key(
                xkb::Keycode::from(keycode),
                match state {
                    KeyState::Pressed => xkb::KeyDirection::Down,
                    KeyState::Released => xkb::KeyDirection::Up,
                },
            );
            
            self.ctrl = xkb_state.mod_name_is_active(
                xkb::MOD_NAME_CTRL,
                xkb::STATE_MODS_EFFECTIVE,
            );
            self.alt = xkb_state.mod_name_is_active(
                xkb::MOD_NAME_ALT,
                xkb::STATE_MODS_EFFECTIVE,
            );
            
            if state == KeyState::Pressed {
                let keysym = xkb_state.key_get_one_sym(xkb::Keycode::from(keycode));
                
                if self.ctrl && self.alt && keysym == KEY_q.into() {
                    self.frame.exit_compositor = true;
                    return;
                } else if self.alt && keysym == KEY_t.into() {
                    self.frame.launch_terminal = true;
                    return;
                } else if self.alt && (keysym == KEY_Tab.into() || keysym == KEY_j.into()) {
                    self.frame.focus_next = true;
                    return;
                } else if self.alt && keysym == KEY_k.into() {
                    self.frame.focus_prev = true;
                    return;
                }
            }
            
            self.frame.keys.push(KeyEvent {
                keycode: keycode - 8,
                state,
                mods_depressed: xkb_state.serialize_mods(xkb::STATE_MODS_DEPRESSED),
                mods_latched: xkb_state.serialize_mods(xkb::STATE_MODS_LATCHED),
                mods_locked: xkb_state.serialize_mods(xkb::STATE_MODS_LOCKED),
                group: xkb_state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE),
            });
        } else {
            log::error!("XKB state unavailable, key event dropped");
        }
    }
    
    fn init_xkb_state(&mut self) {
        let keymap = xkb::Keymap::new_from_names(
            &self.xkb_context,
            "",
            "",
            "",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        );
        
        if let Some(keymap) = keymap {
            self.xkb_state = Some(xkb::State::new(&keymap));
        }
    }
    
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.libinput.as_fd()
    }
}
