// Library crate for Mostrix - exposes modules for testing
pub mod db;
pub mod models;
pub mod settings;
pub mod ui;
pub mod util;

use crate::settings::Settings;
use std::sync::OnceLock;

/// Constructs (or copies) the configuration file and loads it.
pub static SETTINGS: OnceLock<Settings> = OnceLock::new();
