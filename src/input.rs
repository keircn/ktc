use input::event::{Event, EventTrait};
use input::event::keyboard::KeyboardEventTrait;
pub use input::event::keyboard::KeyState;
use input::{Libinput, LibinputInterface};
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use xkbcommon::xkb;
use xkbcommon::xkb::keysyms::{KEY_q, KEY_t};

struct Interface;

impl LibinputInterface for Interface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        OpenOptions::new()
            .custom_flags(flags)
            .read(flags & libc::O_RDWR != 0)
            .write(flags & libc::O_RDWR != 0)
            .open(path)
            .map(|file| file.into())
            .map_err(|err| err.raw_os_error().unwrap_or(-1))
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        drop(File::from(fd));
    }
}

pub struct InputHandler {
    libinput: Libinput,
    xkb_context: xkb::Context,
    xkb_state: Option<xkb::State>,
    ctrl: bool,
    alt: bool,
}

impl InputHandler {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut libinput = Libinput::new_with_udev(Interface);
        libinput.udev_assign_seat("seat0")
            .map_err(|_| "Failed to assign udev seat")?;
        
        let xkb_context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        
        Ok(InputHandler {
            libinput,
            xkb_context,
            xkb_state: None,
            ctrl: false,
            alt: false,
        })
    }
    
    pub fn dispatch(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.libinput.dispatch()?;
        Ok(())
    }
    
    pub fn process_events<F>(&mut self, mut callback: F) 
    where
        F: FnMut(InputAction),
    {
        let mut keyboard_events = Vec::new();
        let mut has_keyboard_device = false;
        let mut event_count = 0;
        
        for event in &mut self.libinput {
            event_count += 1;
            log::info!("Input event received: {:?}", event);
            
            match event {
                Event::Keyboard(keyboard_event) => {
                    let key = keyboard_event.key();
                    let state = keyboard_event.key_state();
                    log::info!("Keyboard event: key={}, state={:?}", key, state);
                    keyboard_events.push((key, state));
                }
                Event::Device(device_event) => {
                    use input::event::DeviceEvent;
                    log::info!("Device event: {:?}", device_event);
                    if let DeviceEvent::Added(added) = device_event {
                        if added.device().has_capability(input::DeviceCapability::Keyboard) {
                            log::info!("Keyboard device added");
                            has_keyboard_device = true;
                        }
                    }
                }
                _ => {
                    log::info!("Other event type");
                }
            }
        }
        
        if event_count == 0 {
            log::info!("No events in libinput queue");
        } else {
            log::info!("Processed {} events total", event_count);
        }
        
        if has_keyboard_device {
            log::info!("Initializing XKB state for new keyboard");
            self.init_xkb_state();
        }
        
        log::info!("Processing {} keyboard events", keyboard_events.len());
        for (key, state) in keyboard_events {
            self.handle_keyboard_key(key, state, &mut callback);
        }
    }
    
    fn init_xkb_state(&mut self) {
        log::info!("Initializing XKB keymap");
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
            log::info!("XKB keymap created successfully");
            self.xkb_state = Some(xkb::State::new(&keymap));
        } else {
            log::error!("Failed to create XKB keymap");
        }
    }
    
    fn handle_keyboard_key<F>(&mut self, key: u32, state: input::event::keyboard::KeyState, callback: &mut F) 
    where
        F: FnMut(InputAction),
    {
        use input::event::keyboard::KeyState;
        
        log::info!("Handling keyboard key: key={}, state={:?}", key, state);
        
        if self.xkb_state.is_none() {
            log::warn!("XKB state not initialized, initializing now");
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
                
                log::info!("Key pressed: keycode={}, keysym={}, ctrl={}, alt={}", 
                    keycode, keysym.raw(), self.ctrl, self.alt);
                
                if self.ctrl && self.alt && keysym == KEY_q.into() {
                    log::info!("Ctrl+Alt+Q detected");
                    callback(InputAction::ExitCompositor);
                    return;
                } else if self.alt && keysym == KEY_t.into() {
                    log::info!("Alt+T detected");
                    callback(InputAction::LaunchTerminal);
                    return;
                }
            }
            
            callback(InputAction::KeyEvent { 
                keycode: keycode - 8, 
                state 
            });
        } else {
            log::error!("XKB state still not available after initialization");
        }
    }
    
    pub fn as_fd(&self) -> BorrowedFd {
        self.libinput.as_fd()
    }
}

pub enum InputAction {
    ExitCompositor,
    LaunchTerminal,
    KeyEvent { keycode: u32, state: KeyState },
}
