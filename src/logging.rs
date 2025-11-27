use log::{Level, LevelFilter, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use chrono::Local;

pub struct FileLogger {
    log_file: Mutex<File>,
}

impl FileLogger {
    pub fn init() -> Result<(), Box<dyn std::error::Error>> {
        let log_dir = Self::determine_log_dir()?;
        fs::create_dir_all(&log_dir)?;
        
        let session_num = Self::get_next_session_number(&log_dir);
        let log_path = log_dir.join(format!("session-{}.log", session_num));
        
        let log_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)?;
        
        let logger = FileLogger {
            log_file: Mutex::new(log_file),
        };
        
        log::set_max_level(LevelFilter::Debug);
        log::set_logger(Box::leak(Box::new(logger)))
            .map_err(|e| format!("Failed to set logger: {}", e))?;
        
        log::info!("=== KTC Session {} ===", session_num);
        log::info!("Log file: {}", log_path.display());
        log::info!("Started at: {}", Local::now().format("%Y-%m-%d %H:%M:%S"));
        
        Ok(())
    }
    
    fn determine_log_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Some(home) = std::env::var_os("HOME") {
            let user_log_dir = PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("ktc")
                .join("logs");
            return Ok(user_log_dir);
        }
        
        let system_log_dir = PathBuf::from("/var/log/ktc");
        if Self::can_write_to_dir(&system_log_dir) {
            return Ok(system_log_dir);
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
    
    fn get_next_session_number(log_dir: &PathBuf) -> u32 {
        let mut max_num = 0u32;
        
        if let Ok(entries) = fs::read_dir(log_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                
                if let Some(rest) = name_str.strip_prefix("session-") {
                    if let Some(num_str) = rest.strip_suffix(".log") {
                        if let Ok(num) = num_str.parse::<u32>() {
                            max_num = max_num.max(num);
                        }
                    }
                }
            }
        }
        
        max_num + 1
    }
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Debug
    }
    
    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        
        let timestamp = Local::now().format("%H:%M:%S%.3f");
        let level_char = match record.level() {
            Level::Error => 'E',
            Level::Warn => 'W',
            Level::Info => 'I',
            Level::Debug => 'D',
            Level::Trace => 'T',
        };
        
        let log_line = format!(
            "{} {} {} {}\n",
            timestamp,
            level_char,
            record.target(),
            record.args()
        );
        
        if let Ok(mut file) = self.log_file.lock() {
            let _ = file.write_all(log_line.as_bytes());
            let _ = file.flush();
        }
        
        // Also print to stderr for immediate visibility
        eprint!("{}", log_line);
    }
    
    fn flush(&self) {
        let _ = self.log_file.lock().map(|mut f| f.flush());
    }
}
