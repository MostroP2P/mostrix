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
    #[serde(alias = "currencies")]
    pub currencies_filter: Vec<String>,
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
            currencies_filter: Vec::new(),
            pow: 0,
            user_mode: "user".to_string(),
        }
    }
}

/// Constructs (or copies) the configuration file and loads it.
/// Returns a reference to the global `SETTINGS`, initializing it on first use.
pub fn init_settings() -> Result<&'static Settings, anyhow::Error> {
    if let Some(settings) = SETTINGS.get() {
        return Ok(settings);
    }

    let settings = init_or_load_settings_from_disk()?;

    // It's fine if another thread initialized SETTINGS first; in that case we just reuse it.
    if SETTINGS.set(settings).is_err() {
        // SETTINGS was already set between the get() above and set() here.
        // Safe to unwrap because we know some value is now present.
        return Ok(SETTINGS.get().expect("SETTINGS should be initialized"));
    }

    Ok(SETTINGS.get().expect("SETTINGS should be initialized"))
}

/// Validates currencies config: exits with a clear error if the deprecated
/// `currencies` field is used or both `currencies` and `currencies_filter` are present.
/// Only considers non-comment lines (key before any `#`).
fn validate_currencies_config(settings_path: &PathBuf) -> Result<(), anyhow::Error> {
    let config_str = match fs::read_to_string(settings_path) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    let mut has_old = false;
    let mut has_new = false;
    for line in config_str.lines() {
        let before_comment = line.split('#').next().unwrap_or(line).trim();
        if before_comment.is_empty() {
            continue;
        }
        if before_comment.starts_with("currencies_filter =")
            || before_comment.starts_with("currencies_filter=")
        {
            has_new = true;
        } else if before_comment.starts_with("currencies =")
            || before_comment.starts_with("currencies=")
        {
            has_old = true;
        }
    }

    let path_display = settings_path.display();
    if has_old && !has_new {
        anyhow::bail!(
            "Deprecated field 'currencies' in {}. Please rename to 'currencies_filter' and run again. See README 'Upgrading from v0.x'.",
            path_display
        );
    }
    if has_old && has_new {
        anyhow::bail!(
            "Both 'currencies' and 'currencies_filter' are set in {}. Remove 'currencies' and keep only 'currencies_filter', then run again.",
            path_display
        );
    }
    Ok(())
}

/// Internal helper: ensure settings file exists and load it from disk
fn init_or_load_settings_from_disk() -> Result<Settings, anyhow::Error> {
    // HOME and package name at compile time
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let package_name = env!("CARGO_PKG_NAME");
    let hidden_dir = home_dir.join(format!(".{package_name}"));
    let hidden_file = hidden_dir.join("settings.toml");

    // Path to the settings.toml included in the repo (next to Cargo.toml)
    let default_file: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("settings.toml");

    // Create ~/.mostrix if it doesn't exist
    if !hidden_dir.exists() {
        fs::create_dir(&hidden_dir).map_err(|e| {
            anyhow::anyhow!("The configuration directory could not be created: {}", e)
        })?;
    }

    // Copy settings.toml if it isn't already in ~/.mostrix
    if !hidden_file.exists() {
        fs::copy(&default_file, &hidden_file)
            .map_err(|e| anyhow::anyhow!("Could not copy default settings.toml: {}", e))?;
    }

    validate_currencies_config(&hidden_file)?;

    // Use the `config` crate to deserialize to the Settings struct
    let cfg = config::Config::builder()
        .add_source(config::File::from(hidden_file.as_path()))
        .build()
        .map_err(|e| anyhow::anyhow!("settings.toml malformed: {}", e))?;

    cfg.try_deserialize::<Settings>()
        .map_err(|e| anyhow::anyhow!("Error deserializing settings.toml: {}", e))
}

/// Public helper: reload current settings from disk (reflects all previous saves)
pub fn load_settings_from_disk() -> Result<Settings, anyhow::Error> {
    init_or_load_settings_from_disk()
}

/// Save settings to file
pub fn save_settings(settings: &Settings) -> Result<(), anyhow::Error> {
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_alias_loads_old_currencies_field() {
        let toml = r#"
            mostro_pubkey = "npub1test"
            nsec_privkey = "nsec1test"
            admin_privkey = "nsec1admin"
            relays = ["wss://relay.example.com"]
            log_level = "info"
            currencies = ["USD", "EUR"]
            pow = 0
        "#;
        let settings: Settings = toml::from_str(toml).unwrap();
        assert_eq!(settings.currencies_filter, vec!["USD", "EUR"]);
    }
}
