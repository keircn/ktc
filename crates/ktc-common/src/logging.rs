use chrono::Local;
use log::{Level, LevelFilter, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::paths::ktc_log_dir;

static SESSION_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

pub struct FileLogger {
    main_file: Mutex<File>,
    debug_file: Mutex<File>,
}

impl FileLogger {
    pub fn init() -> Result<(), Box<dyn std::error::Error>> {
        let log_dir = ktc_log_dir();
        fs::create_dir_all(&log_dir)?;

        let session_num = Self::get_next_session_number(&log_dir);
        let session_dir = log_dir.join(format!("session-{}", session_num));
        fs::create_dir_all(&session_dir)?;

        if let Ok(mut guard) = SESSION_DIR.lock() {
            *guard = Some(session_dir.clone());
        }

        let main_file = Self::open_log_file(&session_dir, "ktc.log")?;
        let debug_file = Self::open_log_file(&session_dir, "ktc.dbg.log")?;

        let logger = FileLogger {
            main_file: Mutex::new(main_file),
            debug_file: Mutex::new(debug_file),
        };

        log::set_max_level(LevelFilter::Debug);
        log::set_logger(Box::leak(Box::new(logger)))
            .map_err(|e| format!("Failed to set logger: {}", e))?;

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        log::info!("=== KTC Session {} ===", session_num);
        log::info!("Log directory: {}", session_dir.display());
        log::info!("Started at: {}", timestamp);

        Ok(())
    }

    fn open_log_file(dir: &Path, name: &str) -> Result<File, Box<dyn std::error::Error>> {
        let path = dir.join(name);
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
                    if let Ok(num) = rest.parse::<u32>() {
                        max_num = max_num.max(num);
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

        let log_line = format!("{} {} {}\n", timestamp, record.target(), record.args());

        let file_mutex = if record.level() == Level::Debug || record.level() == Level::Trace {
            &self.debug_file
        } else {
            &self.main_file
        };

        if let Ok(mut file) = file_mutex.lock() {
            let _ = file.write_all(log_line.as_bytes());
            let _ = file.flush();
        }

        eprint!("{} {} {}", timestamp, level_char, log_line);
    }

    fn flush(&self) {
        let _ = self.main_file.lock().map(|mut f| f.flush());
        let _ = self.debug_file.lock().map(|mut f| f.flush());
    }
}

pub fn current_session_dir() -> Option<PathBuf> {
    SESSION_DIR.lock().ok()?.clone()
}

pub struct AppLogger {
    file: Mutex<File>,
    app_name: String,
}

impl AppLogger {
    pub fn init(app_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let session_dir = Self::find_current_session_dir()?;
        let log_path = session_dir.join(format!("{}.log", app_name));

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        let logger = AppLogger {
            file: Mutex::new(file),
            app_name: app_name.to_string(),
        };

        log::set_max_level(LevelFilter::Debug);
        log::set_logger(Box::leak(Box::new(logger)))
            .map_err(|e| format!("Failed to set logger: {}", e))?;

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        log::info!("=== {} started at {} ===", app_name, timestamp);

        Ok(())
    }

    fn find_current_session_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let log_dir = ktc_log_dir();

        let mut latest_session: Option<(u32, PathBuf)> = None;

        if let Ok(entries) = fs::read_dir(&log_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                if let Some(rest) = name_str.strip_prefix("session-") {
                    if let Ok(num) = rest.parse::<u32>() {
                        if latest_session.is_none() || num > latest_session.as_ref().unwrap().0 {
                            latest_session = Some((num, entry.path()));
                        }
                    }
                }
            }
        }

        latest_session
            .map(|(_, path)| path)
            .ok_or_else(|| "No KTC session found".into())
    }
}

impl log::Log for AppLogger {
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

        let log_line = format!("{} {} {}\n", timestamp, record.target(), record.args());

        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_all(log_line.as_bytes());
            let _ = file.flush();
        }

        eprint!(
            "[{}] {} {} {}",
            self.app_name, timestamp, level_char, log_line
        );
    }

    fn flush(&self) {
        let _ = self.file.lock().map(|mut f| f.flush());
    }
}
