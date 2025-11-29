pub mod color;
pub mod logging;
pub mod paths;

pub use color::parse_color;
pub use logging::FileLogger;
pub use paths::{config_dir, data_dir, ktc_config_dir, ktc_data_dir, ktc_log_dir};
