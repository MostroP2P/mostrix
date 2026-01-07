use crate::SETTINGS;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub mostro_pubkey: String,
    pub nsec_privkey: String,
    pub admin_privkey: String,
    pub relays: Vec<String>,
    pub log_level: String,
    pub currencies: Vec<String>,
    pub pow: u8,
    #[serde(default = "default_user_mode")]
    pub user_mode: String, // "user" or "admin", default "user"
}

fn default_user_mode() -> String {
    "user".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mostro_pubkey: String::new(),
            nsec_privkey: String::new(),
            admin_privkey: String::new(),
            relays: Vec::new(),
            log_level: "info".to_string(),
            currencies: Vec::new(),
            pow: 0,
            user_mode: "user".to_string(),
        }
    }
}

/// Constructs (or copies) the configuration file and loads it
pub fn init_settings() -> &'static Settings {
    SETTINGS.get_or_init(|| {
        // HOME and package name at compile time
        let home_dir = dirs::home_dir().expect("Could not find home directory");
        let package_name = env!("CARGO_PKG_NAME");
        let hidden_dir = home_dir.join(format!(".{package_name}"));
        let hidden_file = hidden_dir.join("settings.toml");

        println!("hidden_file: {:?}", hidden_file);

        // Path to the settings.toml included in the repo (next to Cargo.toml)
        let default_file: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("settings.toml");

        // Create ~/.mostrix if it doesn't exist
        if !hidden_dir.exists() {
            fs::create_dir(&hidden_dir).expect("The configuration directory could not be created");
        }

        // Copy settings.toml if it isn't already in ~/.mostrix
        if !hidden_file.exists() {
            fs::copy(&default_file, &hidden_file).expect("Could not copy default settings.toml");
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

/// Save settings to file
pub fn save_settings(settings: &Settings) -> Result<(), anyhow::Error> {
    let home_dir = dirs::home_dir().expect("Could not find home directory");
    let package_name = env!("CARGO_PKG_NAME");
    let hidden_file = home_dir
        .join(format!(".{package_name}"))
        .join("settings.toml");

    let toml_string = toml::to_string_pretty(settings)
        .map_err(|e| anyhow::anyhow!("Failed to serialize settings: {}", e))?;

    fs::write(&hidden_file, toml_string)
        .map_err(|e| anyhow::anyhow!("Failed to write settings file: {}", e))?;

    Ok(())
}
