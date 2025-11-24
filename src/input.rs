use input::event::{Event, EventTrait};
use input::event::keyboard::KeyboardEventTrait;
use input::{Libinput, LibinputInterface};
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use xkbcommon::xkb;
use xkbcommon::xkb::keysyms::{KEY_Delete, KEY_t};

struct Interface;

impl LibinputInterface for Interface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        OpenOptions::new()
            .custom_flags(flags)
            .read((flags & libc::O_RDWR != 0) || (flags & libc::O_RDONLY != 0))
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
        
        for event in &mut self.libinput {
            match event {
                Event::Keyboard(keyboard_event) => {
                    keyboard_events.push((keyboard_event.key(), keyboard_event.key_state()));
                }
                Event::Device(device_event) => {
                    use input::event::DeviceEvent;
                    if let DeviceEvent::Added(added) = device_event {
                        if added.device().has_capability(input::DeviceCapability::Keyboard) {
                            has_keyboard_device = true;
                        }
                    }
                }
                _ => {}
            }
        }
        
        if has_keyboard_device {
            self.init_xkb_state();
        }
        
        for (key, state) in keyboard_events {
            self.handle_keyboard_key(key, state, &mut callback);
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
    
    fn handle_keyboard_key<F>(&mut self, key: u32, state: input::event::keyboard::KeyState, callback: &mut F) 
    where
        F: FnMut(InputAction),
    {
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
                
                if self.ctrl && self.alt && keysym == KEY_Delete.into() {
                    callback(InputAction::ExitCompositor);
                } else if self.alt && keysym == KEY_t.into() {
                    callback(InputAction::LaunchTerminal);
                }
            }
        }
    }
    
    pub fn as_fd(&self) -> BorrowedFd {
        self.libinput.as_fd()
    }
}

pub enum InputAction {
    ExitCompositor,
    LaunchTerminal,
}
