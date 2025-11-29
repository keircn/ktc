use ktc_common::{parse_color, ktc_config_dir};
use serde::Deserialize;
use std::path::PathBuf;

fn default_title_bar_height() -> i32 { 24 }
fn default_border_width() -> i32 { 1 }
fn default_gap() -> i32 { 0 }

fn default_background_dark() -> String { "#1A1A2E".to_string() }
fn default_background_light() -> String { "#16213E".to_string() }
fn default_title_focused() -> String { "#2D5A88".to_string() }
fn default_title_unfocused() -> String { "#3C3C3C".to_string() }
fn default_border_focused() -> String { "#4A9EFF".to_string() }
fn default_border_unfocused() -> String { "#505050".to_string() }

fn default_keyboard_layout() -> String { "us".to_string() }
fn default_keyboard_model() -> String { "pc105".to_string() }
fn default_keyboard_options() -> String { String::new() }
fn default_repeat_rate() -> i32 { 25 }
fn default_repeat_delay() -> i32 { 600 }

fn default_cursor_theme() -> String { "default".to_string() }
fn default_cursor_size() -> i32 { 24 }

fn default_drm_device() -> String { "auto".to_string() }
fn default_preferred_mode() -> String { "auto".to_string() }
fn default_vsync() -> bool { true }
fn default_vrr() -> bool { false }
fn default_gpu() -> bool { true }

fn default_mod_key() -> String { "alt".to_string() }

fn default_bindings() -> Vec<KeybindEntry> {
    vec![
        KeybindEntry { key: "ctrl+alt+q".to_string(), action: "exit".to_string() },
        KeybindEntry { key: "mod+Return".to_string(), action: "exec foot".to_string() },
        KeybindEntry { key: "mod+j".to_string(), action: "focus_next".to_string() },
        KeybindEntry { key: "mod+k".to_string(), action: "focus_prev".to_string() },
        KeybindEntry { key: "mod+shift+q".to_string(), action: "close_window".to_string() },
        KeybindEntry { key: "mod+1".to_string(), action: "workspace 1".to_string() },
        KeybindEntry { key: "mod+2".to_string(), action: "workspace 2".to_string() },
        KeybindEntry { key: "mod+3".to_string(), action: "workspace 3".to_string() },
        KeybindEntry { key: "mod+4".to_string(), action: "workspace 4".to_string() },
        KeybindEntry { key: "mod+shift+1".to_string(), action: "move_to_workspace 1".to_string() },
        KeybindEntry { key: "mod+shift+2".to_string(), action: "move_to_workspace 2".to_string() },
        KeybindEntry { key: "mod+shift+3".to_string(), action: "move_to_workspace 3".to_string() },
        KeybindEntry { key: "mod+shift+4".to_string(), action: "move_to_workspace 4".to_string() },
    ]
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
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            device: default_drm_device(),
            mode: default_preferred_mode(),
            vsync: default_vsync(),
            vrr: default_vrr(),
            gpu: default_gpu(),
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
        let refresh = parts.get(1).and_then(|r| r.trim_end_matches("Hz").parse().ok());
        
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
                "super" | "mod4" | "logo" => super_key = true,
                "mod" => {
                    match self.mod_key.to_lowercase().as_str() {
                        "alt" => alt = true,
                        "super" | "mod4" | "logo" => super_key = true,
                        "ctrl" | "control" => ctrl = true,
                        _ => alt = true,
                    }
                }
                _ => key_part = Box::leak(part.into_boxed_str()),
            }
        }
        
        let keysym = keysym_from_name(key_part)?;
        Some(Keybind { ctrl, alt, shift, super_key, keysym })
    }
    
    pub fn get_all_bindings(&self) -> Vec<(String, Keybind)> {
        self.bind.iter()
            .filter_map(|entry| {
                self.parse_keybind(&entry.key).map(|kb| (entry.action.clone(), kb))
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
        "return" | "enter" => KEY_Return,
        "escape" | "esc" => KEY_Escape,
        "tab" => KEY_Tab,
        "space" => KEY_space,
        "backspace" => KEY_BackSpace,
        "delete" => KEY_Delete,
        "insert" => KEY_Insert,
        "home" => KEY_Home,
        "end" => KEY_End,
        "pageup" | "page_up" => KEY_Page_Up,
        "pagedown" | "page_down" => KEY_Page_Down,
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
        "equal" | "=" => KEY_equal,
        "bracketleft" | "[" => KEY_bracketleft,
        "bracketright" | "]" => KEY_bracketright,
        "semicolon" | ";" => KEY_semicolon,
        "apostrophe" | "'" => KEY_apostrophe,
        "grave" | "`" => KEY_grave,
        "backslash" | "\\" => KEY_backslash,
        "comma" | "," => KEY_comma,
        "period" | "." => KEY_period,
        "slash" | "/" => KEY_slash,
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
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file: {}", e))?;
        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse TOML: {}", e))
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
