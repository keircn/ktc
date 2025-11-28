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

fn default_mod_key() -> String { "alt".to_string() }
fn default_focus_next() -> String { "mod+j".to_string() }
fn default_focus_prev() -> String { "mod+k".to_string() }
fn default_close_window() -> String { "mod+shift+q".to_string() }
fn default_exit() -> String { "ctrl+alt+q".to_string() }

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub appearance: AppearanceConfig,
    pub keyboard: KeyboardConfig,
    pub cursor: CursorConfig,
    pub keybinds: KeybindsConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct AppearanceConfig {
    #[serde(default = "default_title_bar_height")]
    pub title_bar_height: i32,
    #[serde(default = "default_border_width")]
    pub border_width: i32,
    #[serde(default = "default_gap")]
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
    pub repeat_rate: i32,
    #[serde(default = "default_repeat_delay")]
    pub repeat_delay: i32,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
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
    #[serde(default = "default_focus_next")]
    pub focus_next: String,
    #[serde(default = "default_focus_prev")]
    pub focus_prev: String,
    #[serde(default = "default_close_window")]
    pub close_window: String,
    #[serde(default = "default_exit")]
    pub exit: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            appearance: AppearanceConfig::default(),
            keyboard: KeyboardConfig::default(),
            cursor: CursorConfig::default(),
            keybinds: KeybindsConfig::default(),
        }
    }
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

impl Default for KeybindsConfig {
    fn default() -> Self {
        Self {
            mod_key: default_mod_key(),
            focus_next: default_focus_next(),
            focus_prev: default_focus_prev(),
            close_window: default_close_window(),
            exit: default_exit(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let user_config = dirs_user_config().join("ktc/config.toml");
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

fn dirs_user_config() -> PathBuf {
    if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg_config);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config");
    }
    PathBuf::from("/tmp")
}

fn parse_color(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('#');
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    } else if s.len() == 8 {
        let a = u8::from_str_radix(&s[0..2], 16).ok()?;
        let r = u8::from_str_radix(&s[2..4], 16).ok()?;
        let g = u8::from_str_radix(&s[4..6], 16).ok()?;
        let b = u8::from_str_radix(&s[6..8], 16).ok()?;
        Some(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    } else {
        None
    }
}
