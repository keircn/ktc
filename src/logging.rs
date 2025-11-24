use log::{Level, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use chrono::Local;

pub struct FileLogger {
    session_id: String,
    log_dir: PathBuf,
    error_file: Mutex<File>,
    warn_file: Mutex<File>,
    info_file: Mutex<File>,
}

impl FileLogger {
    pub fn init() -> Result<(), Box<dyn std::error::Error>> {
        let session_id = Local::now().format("%Y%m%d_%H%M%S").to_string();
        
        let log_dir = Self::determine_log_dir()?;
        fs::create_dir_all(&log_dir)?;
        
        let error_file = Self::open_log_file(&log_dir, &session_id, "error")?;
        let warn_file = Self::open_log_file(&log_dir, &session_id, "warn")?;
        let info_file = Self::open_log_file(&log_dir, &session_id, "info")?;
        
        let logger = FileLogger {
            session_id: session_id.clone(),
            log_dir: log_dir.clone(),
            error_file: Mutex::new(error_file),
            warn_file: Mutex::new(warn_file),
            info_file: Mutex::new(info_file),
        };
        
        log::set_max_level(log::LevelFilter::Info);
        log::set_logger(Box::leak(Box::new(logger)))
            .map_err(|e| format!("Failed to set logger: {}", e))?;
        
        log::info!("KTC compositor started - session {}", session_id);
        log::info!("Logs directory: {}", log_dir.display());
        
        Ok(())
    }
    
    fn determine_log_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let system_log_dir = PathBuf::from("/var/log/ktc");
        
        if Self::can_write_to_dir(&system_log_dir) {
            return Ok(system_log_dir);
        }
        
        if let Some(home) = std::env::var_os("HOME") {
            let user_log_dir = PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("ktc")
                .join("logs");
            return Ok(user_log_dir);
        }
        
        Err("Could not determine log directory".into())
    }
    
    fn can_write_to_dir(path: &PathBuf) -> bool {
        if path.exists() {
            return path.metadata()
                .map(|m| !m.permissions().readonly())
                .unwrap_or(false);
        }
        
        fs::create_dir_all(path).is_ok()
    }
    
    fn open_log_file(log_dir: &PathBuf, session_id: &str, level: &str) -> Result<File, std::io::Error> {
        let filename = format!("{}_{}.log", session_id, level);
        let path = log_dir.join(filename);
        
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    }
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }
    
    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let log_line = format!(
            "[{}] [{}] [{}:{}] {}\n",
            timestamp,
            record.level(),
            record.file().unwrap_or("unknown"),
            record.line().unwrap_or(0),
            record.args()
        );
        
        let file_mutex = match record.level() {
            Level::Error => &self.error_file,
            Level::Warn => &self.warn_file,
            Level::Info => &self.info_file,
            _ => &self.info_file,
        };
        
        if let Ok(mut file) = file_mutex.lock() {
            let _ = file.write_all(log_line.as_bytes());
            let _ = file.flush();
        }
        
        eprintln!("{}", log_line.trim_end());
    }
    
    fn flush(&self) {
        let _ = self.error_file.lock().map(|mut f| f.flush());
        let _ = self.warn_file.lock().map(|mut f| f.flush());
        let _ = self.info_file.lock().map(|mut f| f.flush());
    }
}
