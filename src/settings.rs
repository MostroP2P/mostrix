use crate::SETTINGS;

use serde::Deserialize;
use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub mostro_pubkey: String,
    pub relays:        Vec<String>,
    pub log_level:     String,
}

/// Constructs (or copies) the configuration file and loads it
pub fn init_settings() -> &'static Settings {
    SETTINGS.get_or_init(|| {
        // HOME and package name at compile time
        let home_dir = dirs::home_dir().expect("Could not find home directory");
        let package_name  = env!("CARGO_PKG_NAME");          // p.e. "my_project"
        let hidden_dir    = home_dir.join(format!(".{package_name}"));
        let hidden_file   = hidden_dir.join("settings.toml");

        // Path to the settings.toml included in the repo (next to Cargo.toml)
        let default_file: PathBuf =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("settings.toml");

        // Create ~/.mostrix if it doesn't exist
        if !hidden_dir.exists() {
            fs::create_dir(&hidden_dir)
                .expect("The configuration directory could not be created");
        }

        // Copy settings.toml if it isn't already in ~/.mostrix
        if !hidden_file.exists() {
            fs::copy(&default_file, &hidden_file)
                .expect("Could not copy default settings.toml");
        }

        // Use the `config` crate to deserialize to the Settings struct
        let cfg = config::Config::builder()
            .add_source(config::File::from(hidden_file))
            .build()
            .expect("settings.toml malformed");

        cfg.try_deserialize::<Settings>()
            .expect("Error deserializing settings.toml")
    })
}