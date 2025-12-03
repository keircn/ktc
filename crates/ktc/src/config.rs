use ktc_common::{ktc_config_dir, parse_color};
use serde::Deserialize;
use std::path::PathBuf;

fn default_title_bar_height() -> i32 {
    24
}
fn default_border_width() -> i32 {
    1
}
fn default_gap() -> i32 {
    0
}

fn default_background_dark() -> String {
    "#1A1A2E".to_string()
}
fn default_background_light() -> String {
    "#16213E".to_string()
}
fn default_title_focused() -> String {
    "#2D5A88".to_string()
}
fn default_title_unfocused() -> String {
    "#3C3C3C".to_string()
}
fn default_border_focused() -> String {
    "#4A9EFF".to_string()
}
fn default_border_unfocused() -> String {
    "#505050".to_string()
}

fn default_keyboard_layout() -> String {
    "us".to_string()
}
fn default_keyboard_model() -> String {
    "pc105".to_string()
}
fn default_keyboard_options() -> String {
    String::new()
}
fn default_repeat_rate() -> i32 {
    25
}
fn default_repeat_delay() -> i32 {
    600
}

fn default_cursor_theme() -> String {
    "default".to_string()
}
fn default_cursor_size() -> i32 {
    24
}

fn default_drm_device() -> String {
    "auto".to_string()
}
fn default_preferred_mode() -> String {
    "auto".to_string()
}
fn default_vsync() -> bool {
    true
}
fn default_vrr() -> bool {
    false
}
fn default_gpu() -> bool {
    true
}

fn default_renderer() -> String {
    "opengl".to_string()
}

fn default_mod_key() -> String {
    "alt".to_string()
}

fn default_bindings() -> Vec<KeybindEntry> {
    vec![
        KeybindEntry {
            key: "ctrl+alt+q".to_string(),
            action: "exit".to_string(),
        },
        KeybindEntry {
            key: "mod+Return".to_string(),
            action: "exec foot".to_string(),
        },
        KeybindEntry {
            key: "mod+d".to_string(),
            action: "exec fuzzel".to_string(),
        },
        KeybindEntry {
            key: "mod+j".to_string(),
            action: "focus next".to_string(),
        },
        KeybindEntry {
            key: "mod+k".to_string(),
            action: "focus prev".to_string(),
        },
        KeybindEntry {
            key: "mod+h".to_string(),
            action: "focus left".to_string(),
        },
        KeybindEntry {
            key: "mod+l".to_string(),
            action: "focus right".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+j".to_string(),
            action: "move next".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+k".to_string(),
            action: "move prev".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+q".to_string(),
            action: "close".to_string(),
        },
        KeybindEntry {
            key: "mod+f".to_string(),
            action: "fullscreen".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+space".to_string(),
            action: "floating toggle".to_string(),
        },
        KeybindEntry {
            key: "mod+1".to_string(),
            action: "workspace 1".to_string(),
        },
        KeybindEntry {
            key: "mod+2".to_string(),
            action: "workspace 2".to_string(),
        },
        KeybindEntry {
            key: "mod+3".to_string(),
            action: "workspace 3".to_string(),
        },
        KeybindEntry {
            key: "mod+4".to_string(),
            action: "workspace 4".to_string(),
        },
        KeybindEntry {
            key: "mod+5".to_string(),
            action: "workspace 5".to_string(),
        },
        KeybindEntry {
            key: "mod+6".to_string(),
            action: "workspace 6".to_string(),
        },
        KeybindEntry {
            key: "mod+7".to_string(),
            action: "workspace 7".to_string(),
        },
        KeybindEntry {
            key: "mod+8".to_string(),
            action: "workspace 8".to_string(),
        },
        KeybindEntry {
            key: "mod+9".to_string(),
            action: "workspace 9".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+1".to_string(),
            action: "move_to_workspace 1".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+2".to_string(),
            action: "move_to_workspace 2".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+3".to_string(),
            action: "move_to_workspace 3".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+4".to_string(),
            action: "move_to_workspace 4".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+5".to_string(),
            action: "move_to_workspace 5".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+6".to_string(),
            action: "move_to_workspace 6".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+7".to_string(),
            action: "move_to_workspace 7".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+8".to_string(),
            action: "move_to_workspace 8".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+9".to_string(),
            action: "move_to_workspace 9".to_string(),
        },
        KeybindEntry {
            key: "mod+shift+c".to_string(),
            action: "reload".to_string(),
        },
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
    Next,
    Prev,
}

impl Direction {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "left" | "l" => Some(Direction::Left),
            "right" | "r" => Some(Direction::Right),
            "up" | "u" => Some(Direction::Up),
            "down" | "d" => Some(Direction::Down),
            "next" | "n" => Some(Direction::Next),
            "prev" | "previous" | "p" => Some(Direction::Prev),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResizeDirection {
    Grow,
    Shrink,
    Left,
    Right,
    Up,
    Down,
}

impl ResizeDirection {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "grow" | "+" => Some(ResizeDirection::Grow),
            "shrink" | "-" => Some(ResizeDirection::Shrink),
            "left" | "l" => Some(ResizeDirection::Left),
            "right" | "r" => Some(ResizeDirection::Right),
            "up" | "u" => Some(ResizeDirection::Up),
            "down" | "d" => Some(ResizeDirection::Down),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToggleState {
    On,
    Off,
    Toggle,
}

impl ToggleState {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "on" | "true" | "enable" | "yes" | "1" => Some(ToggleState::On),
            "off" | "false" | "disable" | "no" | "0" => Some(ToggleState::Off),
            "toggle" | "t" => Some(ToggleState::Toggle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceTarget {
    Number(usize),
    Next,
    Prev,
    First,
    Last,
    Empty,
}

impl WorkspaceTarget {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "next" | "n" | "+1" => Some(WorkspaceTarget::Next),
            "prev" | "previous" | "p" | "-1" => Some(WorkspaceTarget::Prev),
            "first" | "1st" => Some(WorkspaceTarget::First),
            "last" => Some(WorkspaceTarget::Last),
            "empty" | "e" => Some(WorkspaceTarget::Empty),
            s => s.parse::<usize>().ok().map(WorkspaceTarget::Number),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Exit,
    Reload,

    Exec(String),
    ExecSpawn(String),

    Close,
    Kill,

    Focus(Direction),
    Move(Direction),
    Swap(Direction),

    Fullscreen(ToggleState),
    Floating(ToggleState),
    Maximize(ToggleState),

    Resize {
        direction: ResizeDirection,
        amount: i32,
    },

    Workspace(WorkspaceTarget),
    MoveToWorkspace(WorkspaceTarget),
    MoveToWorkspaceSilent(WorkspaceTarget),

    SplitHorizontal,
    SplitVertical,
    SplitToggle,

    LayoutNext,
    LayoutPrev,
    LayoutSet(String),

    CursorTheme(String),
}

impl Action {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (cmd, args) = match s.find(' ') {
            Some(i) => (s[..i].trim(), s[i + 1..].trim()),
            None => (s, ""),
        };

        match cmd.to_lowercase().as_str() {
            "exit" | "quit" => Some(Action::Exit),
            "reload" | "reload_config" => Some(Action::Reload),

            "exec" => {
                if args.is_empty() {
                    None
                } else {
                    Some(Action::Exec(args.to_string()))
                }
            }
            "exec_spawn" | "spawn" => {
                if args.is_empty() {
                    None
                } else {
                    Some(Action::ExecSpawn(args.to_string()))
                }
            }

            "close" | "close_window" => Some(Action::Close),
            "kill" | "kill_window" | "killactive" => Some(Action::Kill),

            "focus" | "focus_window" => {
                if args.is_empty() {
                    Some(Action::Focus(Direction::Next))
                } else {
                    Direction::parse(args).map(Action::Focus)
                }
            }
            "focus_next" => Some(Action::Focus(Direction::Next)),
            "focus_prev" => Some(Action::Focus(Direction::Prev)),

            "move" | "move_window" | "movewindow" => {
                if args.is_empty() {
                    Some(Action::Move(Direction::Next))
                } else {
                    Direction::parse(args).map(Action::Move)
                }
            }

            "swap" | "swap_window" | "swapwindow" => {
                if args.is_empty() {
                    Some(Action::Swap(Direction::Next))
                } else {
                    Direction::parse(args).map(Action::Swap)
                }
            }

            "fullscreen" | "togglefullscreen" => {
                if args.is_empty() {
                    Some(Action::Fullscreen(ToggleState::Toggle))
                } else {
                    ToggleState::parse(args).map(Action::Fullscreen)
                }
            }

            "floating" | "togglefloating" => {
                if args.is_empty() {
                    Some(Action::Floating(ToggleState::Toggle))
                } else {
                    ToggleState::parse(args).map(Action::Floating)
                }
            }

            "maximize" | "togglemaximize" => {
                if args.is_empty() {
                    Some(Action::Maximize(ToggleState::Toggle))
                } else {
                    ToggleState::parse(args).map(Action::Maximize)
                }
            }

            "resize" | "resizeactive" => {
                let parts: Vec<&str> = args.split_whitespace().collect();
                if parts.len() >= 2 {
                    let dir = ResizeDirection::parse(parts[0])?;
                    let amount = parts[1].parse().unwrap_or(10);
                    Some(Action::Resize {
                        direction: dir,
                        amount,
                    })
                } else if parts.len() == 1 {
                    let dir = ResizeDirection::parse(parts[0])?;
                    Some(Action::Resize {
                        direction: dir,
                        amount: 10,
                    })
                } else {
                    None
                }
            }

            "workspace" | "switch_workspace" => {
                if args.is_empty() {
                    None
                } else {
                    WorkspaceTarget::parse(args).map(Action::Workspace)
                }
            }

            "move_to_workspace" | "movetoworkspace" => {
                if args.is_empty() {
                    None
                } else {
                    WorkspaceTarget::parse(args).map(Action::MoveToWorkspace)
                }
            }

            "move_to_workspace_silent" | "movetoworkspacesilent" => {
                if args.is_empty() {
                    None
                } else {
                    WorkspaceTarget::parse(args).map(Action::MoveToWorkspaceSilent)
                }
            }

            "split_horizontal" | "splith" => Some(Action::SplitHorizontal),
            "split_vertical" | "splitv" => Some(Action::SplitVertical),
            "split_toggle" | "splitt" => Some(Action::SplitToggle),

            "layout_next" | "layout" if args == "next" => Some(Action::LayoutNext),
            "layout_prev" | "layout" if args == "prev" => Some(Action::LayoutPrev),
            "layout_set" | "layout" => {
                if args.is_empty() {
                    None
                } else {
                    Some(Action::LayoutSet(args.to_string()))
                }
            }

            "cursor_theme" | "setcursor" => {
                if args.is_empty() {
                    None
                } else {
                    Some(Action::CursorTheme(args.to_string()))
                }
            }

            _ => None,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct KeybindEntry {
    pub key: String,
    pub action: String,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Config {
    pub appearance: AppearanceConfig,
    pub display: DisplayConfig,
    pub keyboard: KeyboardConfig,
    pub cursor: CursorConfig,
    pub keybinds: KeybindsConfig,
    pub debug: DebugConfig,
}

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(default)]
pub struct DebugConfig {
    #[serde(default)]
    pub profiler: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct DisplayConfig {
    #[serde(default = "default_drm_device")]
    pub device: String,

    #[serde(default = "default_preferred_mode")]
    pub mode: String,

    #[serde(default = "default_vsync")]
    pub vsync: bool,

    #[serde(default = "default_vrr")]
    #[allow(dead_code)]
    pub vrr: bool,

    #[serde(default = "default_gpu")]
    pub gpu: bool,

    #[serde(default = "default_renderer")]
    #[allow(dead_code)]
    pub renderer: String,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            device: default_drm_device(),
            mode: default_preferred_mode(),
            vsync: default_vsync(),
            vrr: default_vrr(),
            gpu: default_gpu(),
            renderer: default_renderer(),
        }
    }
}

impl DisplayConfig {
    pub fn drm_device_path(&self) -> Option<String> {
        match self.device.as_str() {
            "auto" | "" => None,
            path => Some(path.to_string()),
        }
    }

    pub fn parse_mode(&self) -> Option<(u16, u16, Option<u32>)> {
        if self.mode == "auto" || self.mode.is_empty() {
            return None;
        }

        let parts: Vec<&str> = self.mode.split('@').collect();
        let resolution = parts.first()?;
        let refresh = parts
            .get(1)
            .and_then(|r| r.trim_end_matches("Hz").parse().ok());

        let dims: Vec<&str> = resolution.split('x').collect();
        if dims.len() != 2 {
            return None;
        }

        let width: u16 = dims[0].parse().ok()?;
        let height: u16 = dims[1].parse().ok()?;

        Some((width, height, refresh))
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct AppearanceConfig {
    #[serde(default = "default_title_bar_height")]
    pub title_bar_height: i32,
    #[serde(default = "default_border_width")]
    #[allow(dead_code)]
    pub border_width: i32,
    #[serde(default = "default_gap")]
    #[allow(dead_code)]
    pub gap: i32,
    #[serde(default = "default_background_dark")]
    pub background_dark: String,
    #[serde(default = "default_background_light")]
    pub background_light: String,
    #[serde(default = "default_title_focused")]
    pub title_focused: String,
    #[serde(default = "default_title_unfocused")]
    pub title_unfocused: String,
    #[serde(default = "default_border_focused")]
    pub border_focused: String,
    #[serde(default = "default_border_unfocused")]
    pub border_unfocused: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct KeyboardConfig {
    #[serde(default = "default_keyboard_layout")]
    pub layout: String,
    #[serde(default = "default_keyboard_model")]
    pub model: String,
    #[serde(default = "default_keyboard_options")]
    pub options: String,
    #[serde(default = "default_repeat_rate")]
    #[allow(dead_code)]
    pub repeat_rate: i32,
    #[serde(default = "default_repeat_delay")]
    #[allow(dead_code)]
    pub repeat_delay: i32,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
#[allow(dead_code)]
pub struct CursorConfig {
    #[serde(default = "default_cursor_theme")]
    pub theme: String,
    #[serde(default = "default_cursor_size")]
    pub size: i32,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct KeybindsConfig {
    #[serde(default = "default_mod_key")]
    pub mod_key: String,

    #[serde(default = "default_bindings")]
    pub bind: Vec<KeybindEntry>,
}

impl Default for KeybindsConfig {
    fn default() -> Self {
        Self {
            mod_key: default_mod_key(),
            bind: default_bindings(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Keybind {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub keysym: u32,
}

impl KeybindsConfig {
    pub fn parse_keybind(&self, bind_str: &str) -> Option<Keybind> {
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut super_key = false;
        let mut key_part = "";

        for part in bind_str.split('+') {
            let part = part.trim().to_lowercase();
            match part.as_str() {
                "ctrl" | "control" => ctrl = true,
                "alt" => alt = true,
                "shift" => shift = true,
                "super" | "mod4" | "logo" | "win" | "meta" => super_key = true,
                "mod" => match self.mod_key.to_lowercase().as_str() {
                    "alt" => alt = true,
                    "super" | "mod4" | "logo" | "win" | "meta" => super_key = true,
                    "ctrl" | "control" => ctrl = true,
                    _ => alt = true,
                },
                _ => key_part = Box::leak(part.into_boxed_str()),
            }
        }

        let keysym = keysym_from_name(key_part)?;
        Some(Keybind {
            ctrl,
            alt,
            shift,
            super_key,
            keysym,
        })
    }

    pub fn get_all_bindings(&self) -> Vec<(Action, Keybind)> {
        self.bind
            .iter()
            .filter_map(|entry| {
                let keybind = self.parse_keybind(&entry.key)?;
                let action = Action::parse(&entry.action)?;
                Some((action, keybind))
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn get_all_bindings_raw(&self) -> Vec<(String, Keybind)> {
        self.bind
            .iter()
            .filter_map(|entry| {
                self.parse_keybind(&entry.key)
                    .map(|kb| (entry.action.clone(), kb))
            })
            .collect()
    }
}

fn keysym_from_name(name: &str) -> Option<u32> {
    use xkbcommon::xkb::keysyms::*;

    Some(match name.to_lowercase().as_str() {
        "a" => KEY_a,
        "b" => KEY_b,
        "c" => KEY_c,
        "d" => KEY_d,
        "e" => KEY_e,
        "f" => KEY_f,
        "g" => KEY_g,
        "h" => KEY_h,
        "i" => KEY_i,
        "j" => KEY_j,
        "k" => KEY_k,
        "l" => KEY_l,
        "m" => KEY_m,
        "n" => KEY_n,
        "o" => KEY_o,
        "p" => KEY_p,
        "q" => KEY_q,
        "r" => KEY_r,
        "s" => KEY_s,
        "t" => KEY_t,
        "u" => KEY_u,
        "v" => KEY_v,
        "w" => KEY_w,
        "x" => KEY_x,
        "y" => KEY_y,
        "z" => KEY_z,
        "0" => KEY_0,
        "1" => KEY_1,
        "2" => KEY_2,
        "3" => KEY_3,
        "4" => KEY_4,
        "5" => KEY_5,
        "6" => KEY_6,
        "7" => KEY_7,
        "8" => KEY_8,
        "9" => KEY_9,
        "return" | "enter" | "ret" => KEY_Return,
        "escape" | "esc" => KEY_Escape,
        "tab" => KEY_Tab,
        "space" | "spc" => KEY_space,
        "backspace" | "bksp" => KEY_BackSpace,
        "delete" | "del" => KEY_Delete,
        "insert" | "ins" => KEY_Insert,
        "home" => KEY_Home,
        "end" => KEY_End,
        "pageup" | "page_up" | "pgup" | "prior" => KEY_Page_Up,
        "pagedown" | "page_down" | "pgdn" | "next" => KEY_Page_Down,
        "left" => KEY_Left,
        "right" => KEY_Right,
        "up" => KEY_Up,
        "down" => KEY_Down,
        "f1" => KEY_F1,
        "f2" => KEY_F2,
        "f3" => KEY_F3,
        "f4" => KEY_F4,
        "f5" => KEY_F5,
        "f6" => KEY_F6,
        "f7" => KEY_F7,
        "f8" => KEY_F8,
        "f9" => KEY_F9,
        "f10" => KEY_F10,
        "f11" => KEY_F11,
        "f12" => KEY_F12,
        "minus" | "-" => KEY_minus,
        "equal" | "=" | "plus" => KEY_equal,
        "bracketleft" | "[" | "lbracket" => KEY_bracketleft,
        "bracketright" | "]" | "rbracket" => KEY_bracketright,
        "semicolon" | ";" => KEY_semicolon,
        "apostrophe" | "'" | "quote" => KEY_apostrophe,
        "grave" | "`" | "backtick" | "tilde" => KEY_grave,
        "backslash" | "\\" | "bslash" => KEY_backslash,
        "comma" | "," => KEY_comma,
        "period" | "." | "dot" => KEY_period,
        "slash" | "/" | "fslash" => KEY_slash,
        "print" | "printscreen" | "prtsc" | "sysrq" => KEY_Print,
        "scrolllock" | "scroll_lock" => KEY_Scroll_Lock,
        "pause" | "break" => KEY_Pause,
        "numlock" | "num_lock" => KEY_Num_Lock,
        "capslock" | "caps_lock" | "caps" => KEY_Caps_Lock,
        "kp_0" | "kp0" => KEY_KP_0,
        "kp_1" | "kp1" => KEY_KP_1,
        "kp_2" | "kp2" => KEY_KP_2,
        "kp_3" | "kp3" => KEY_KP_3,
        "kp_4" | "kp4" => KEY_KP_4,
        "kp_5" | "kp5" => KEY_KP_5,
        "kp_6" | "kp6" => KEY_KP_6,
        "kp_7" | "kp7" => KEY_KP_7,
        "kp_8" | "kp8" => KEY_KP_8,
        "kp_9" | "kp9" => KEY_KP_9,
        "kp_enter" | "kpenter" => KEY_KP_Enter,
        "kp_add" | "kpadd" | "kp_plus" => KEY_KP_Add,
        "kp_subtract" | "kpsubtract" | "kp_minus" => KEY_KP_Subtract,
        "kp_multiply" | "kpmultiply" | "kp_asterisk" => KEY_KP_Multiply,
        "kp_divide" | "kpdivide" | "kp_slash" => KEY_KP_Divide,
        "kp_decimal" | "kpdecimal" | "kp_period" => KEY_KP_Decimal,
        "xf86audioraisevolume" | "volumeup" | "vol_up" => KEY_XF86AudioRaiseVolume,
        "xf86audiolowervolume" | "volumedown" | "vol_down" => KEY_XF86AudioLowerVolume,
        "xf86audiomute" | "mute" => KEY_XF86AudioMute,
        "xf86audioplay" | "play" => KEY_XF86AudioPlay,
        "xf86audiopause" | "pause_media" => KEY_XF86AudioPause,
        "xf86audiostop" | "stop" => KEY_XF86AudioStop,
        "xf86audioprev" | "prev_track" => KEY_XF86AudioPrev,
        "xf86audionext" | "next_track" => KEY_XF86AudioNext,
        "xf86monbrightnessup" | "brightnessup" | "brightness_up" => KEY_XF86MonBrightnessUp,
        "xf86monbrightnessdown" | "brightnessdown" | "brightness_down" => KEY_XF86MonBrightnessDown,
        _ => return None,
    })
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            title_bar_height: default_title_bar_height(),
            border_width: default_border_width(),
            gap: default_gap(),
            background_dark: default_background_dark(),
            background_light: default_background_light(),
            title_focused: default_title_focused(),
            title_unfocused: default_title_unfocused(),
            border_focused: default_border_focused(),
            border_unfocused: default_border_unfocused(),
        }
    }
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            layout: default_keyboard_layout(),
            model: default_keyboard_model(),
            options: default_keyboard_options(),
            repeat_rate: default_repeat_rate(),
            repeat_delay: default_repeat_delay(),
        }
    }
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            theme: default_cursor_theme(),
            size: default_cursor_size(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let user_config = ktc_config_dir().join("config.toml");
        let system_config = PathBuf::from("/etc/ktc/config.toml");

        if user_config.exists() {
            match Self::load_from_path(&user_config) {
                Ok(config) => {
                    log::info!("Loaded config from {}", user_config.display());
                    return config;
                }
                Err(e) => {
                    log::warn!("Failed to load {}: {}", user_config.display(), e);
                }
            }
        }

        if system_config.exists() {
            match Self::load_from_path(&system_config) {
                Ok(config) => {
                    log::info!("Loaded config from {}", system_config.display());
                    return config;
                }
                Err(e) => {
                    log::warn!("Failed to load {}: {}", system_config.display(), e);
                }
            }
        }

        log::info!("Using default configuration");
        Self::default()
    }

    fn load_from_path(path: &PathBuf) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;
        toml::from_str(&content).map_err(|e| format!("Failed to parse TOML: {}", e))
    }

    pub fn title_bar_height(&self) -> i32 {
        self.appearance.title_bar_height
    }

    pub fn background_dark(&self) -> u32 {
        parse_color(&self.appearance.background_dark).unwrap_or(0xFF1A1A2E)
    }

    pub fn background_light(&self) -> u32 {
        parse_color(&self.appearance.background_light).unwrap_or(0xFF16213E)
    }

    pub fn title_focused(&self) -> u32 {
        parse_color(&self.appearance.title_focused).unwrap_or(0xFF2D5A88)
    }

    pub fn title_unfocused(&self) -> u32 {
        parse_color(&self.appearance.title_unfocused).unwrap_or(0xFF3C3C3C)
    }

    pub fn border_focused(&self) -> u32 {
        parse_color(&self.appearance.border_focused).unwrap_or(0xFF4A9EFF)
    }

    pub fn border_unfocused(&self) -> u32 {
        parse_color(&self.appearance.border_unfocused).unwrap_or(0xFF505050)
    }
}
