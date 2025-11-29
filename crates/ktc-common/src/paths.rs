use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg_config);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config");
    }
    PathBuf::from("/tmp")
}

pub fn data_dir() -> PathBuf {
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(xdg_data);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local").join("share");
    }
    PathBuf::from("/tmp")
}

pub fn ktc_config_dir() -> PathBuf {
    config_dir().join("ktc")
}

pub fn ktc_data_dir() -> PathBuf {
    data_dir().join("ktc")
}

pub fn ktc_log_dir() -> PathBuf {
    ktc_data_dir().join("logs")
}
