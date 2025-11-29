use log::{Level, LevelFilter, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use chrono::Local;

use crate::paths::ktc_log_dir;

pub struct FileLogger {
    error_file: Mutex<File>,
    warn_file: Mutex<File>,
    info_file: Mutex<File>,
    debug_file: Mutex<File>,
}

impl FileLogger {
    pub fn init() -> Result<(), Box<dyn std::error::Error>> {
        let log_dir = ktc_log_dir();
        fs::create_dir_all(&log_dir)?;
        
        let session_num = Self::get_next_session_number(&log_dir);
        
        let error_file = Self::open_log_file(&log_dir, session_num, "err")?;
        let warn_file = Self::open_log_file(&log_dir, session_num, "war")?;
        let info_file = Self::open_log_file(&log_dir, session_num, "inf")?;
        let debug_file = Self::open_log_file(&log_dir, session_num, "dbg")?;
        
        let logger = FileLogger {
            error_file: Mutex::new(error_file),
            warn_file: Mutex::new(warn_file),
            info_file: Mutex::new(info_file),
            debug_file: Mutex::new(debug_file),
        };
        
        log::set_max_level(LevelFilter::Debug);
        log::set_logger(Box::leak(Box::new(logger)))
            .map_err(|e| format!("Failed to set logger: {}", e))?;
        
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        log::info!("=== KTC Session {} ===", session_num);
        log::info!("Log directory: {}", log_dir.display());
        log::info!("Started at: {}", timestamp);
        
        Ok(())
    }
    
    fn open_log_file(log_dir: &Path, session_num: u32, suffix: &str) -> Result<File, Box<dyn std::error::Error>> {
        let path = log_dir.join(format!("session-{}.{}.log", session_num, suffix));
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(file)
    }
    
    fn get_next_session_number(log_dir: &Path) -> u32 {
        let mut max_num = 0u32;
        
        if let Ok(entries) = fs::read_dir(log_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                
                if let Some(rest) = name_str.strip_prefix("session-") {
                    if let Some(num_part) = rest.split('.').next() {
                        if let Ok(num) = num_part.parse::<u32>() {
                            max_num = max_num.max(num);
                        }
                    }
                }
            }
        }
        
        max_num + 1
    }
    
    fn get_file_for_level(&self, level: Level) -> &Mutex<File> {
        match level {
            Level::Error => &self.error_file,
            Level::Warn => &self.warn_file,
            Level::Info => &self.info_file,
            Level::Debug | Level::Trace => &self.debug_file,
        }
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
            "{} {} {}\n",
            timestamp,
            record.target(),
            record.args()
        );
        
        let file_mutex = self.get_file_for_level(record.level());
        if let Ok(mut file) = file_mutex.lock() {
            let _ = file.write_all(log_line.as_bytes());
            let _ = file.flush();
        }
        
        eprint!("{} {} {}", timestamp, level_char, log_line);
    }
    
    fn flush(&self) {
        let _ = self.error_file.lock().map(|mut f| f.flush());
        let _ = self.warn_file.lock().map(|mut f| f.flush());
        let _ = self.info_file.lock().map(|mut f| f.flush());
        let _ = self.debug_file.lock().map(|mut f| f.flush());
    }
}
