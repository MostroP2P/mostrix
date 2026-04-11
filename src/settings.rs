use crate::SETTINGS;
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

/// Embedded default `settings.toml` used to bootstrap configuration on first run.
/// This is generated at compile time from the repository root `settings.toml`.
const DEFAULT_SETTINGS_TOML: &str = include_str!("../settings.toml");
pub const MOSTRO_STAGING_PUBKEY: &str =
    "82fa8cb978b43c79b2156585bac2c011176a21d2aead6d9f7c575c005be88390";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub mostro_pubkey: String,
    pub nsec_privkey: String,
    pub admin_privkey: String,
    pub relays: Vec<String>,
    pub log_level: String,
    pub currencies_filter: Vec<String>,
    #[serde(default = "default_user_mode")]
    pub user_mode: String, // "user" or "admin", default "user"
}

fn default_user_mode() -> String {
    "user".to_string()
}

pub struct InitSettingsResult {
    pub settings: &'static Settings,
    /// True when this process generated a brand-new `settings.toml` file
    /// (i.e. "first launch" bootstrap), rather than loading an existing config.
    pub did_generate_new_settings_file: bool,
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
            user_mode: "user".to_string(),
        }
    }
}

/// Constructs (or copies) the configuration file and loads it.
/// Returns a reference to the global `SETTINGS`, initializing it on first use.
pub fn init_settings(identity_keys: Option<Keys>) -> Result<InitSettingsResult, anyhow::Error> {
    if let Some(settings) = SETTINGS.get() {
        return Ok(InitSettingsResult {
            settings,
            did_generate_new_settings_file: false,
        });
    }

    let (settings, did_generate_new_settings_file) =
        init_or_load_settings_from_disk(identity_keys.as_ref())?;

    // It's fine if another thread initialized SETTINGS first; in that case we just reuse it.
    if SETTINGS.set(settings).is_err() {
        // SETTINGS was already set between the get() above and set() here.
        // Safe to unwrap because we know some value is now present.
        return Ok(InitSettingsResult {
            settings: SETTINGS.get().expect("SETTINGS should be initialized"),
            did_generate_new_settings_file: false,
        });
    }

    Ok(InitSettingsResult {
        settings: SETTINGS.get().expect("SETTINGS should be initialized"),
        did_generate_new_settings_file,
    })
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
fn init_or_load_settings_from_disk(
    identity_keys: Option<&Keys>,
) -> Result<(Settings, bool), anyhow::Error> {
    // Legacy location: ~/.mostrix/settings.toml (kept for backwards compatibility).
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let package_name = env!("CARGO_PKG_NAME");
    let hidden_dir = home_dir.join(format!(".{package_name}"));
    let hidden_file = hidden_dir.join("settings.toml");

    // Helper: load a settings file from the given path.
    fn load_settings_from_path(path: &PathBuf) -> Result<Settings, anyhow::Error> {
        validate_currencies_config(path)?;

        let cfg = config::Config::builder()
            .add_source(config::File::from(path.as_path()))
            .build()
            .map_err(|e| anyhow::anyhow!("settings.toml malformed: {}", e))?;

        let settings: Settings = cfg
            .try_deserialize()
            .map_err(|e| anyhow::anyhow!("Error deserializing settings.toml: {}", e))?;

        Ok(settings)
    }

    // Portable install probe: `settings.toml` next to the executable.
    // If present, load it read-only and reuse the same placeholder validation.
    let executable_settings_path: Option<PathBuf> = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("settings.toml")));
    if let Some(path) = &executable_settings_path {
        if path.exists() {
            let settings = load_settings_from_path(path)?;
            if settings.mostro_pubkey == "mostro_pubkey_hex_format"
                || settings.nsec_privkey == "nsec1_privkey_format"
            {
                let path_display = path.display();
                anyhow::bail!(
                    "Default settings.toml already exists at {} but still contains placeholder values.\n\
Please edit this file and replace placeholder values (mostro_pubkey, nsec_privkey, etc.) \
with your real keys before running Mostrix again.",
                    path_display
                );
            }

            return Ok((settings, false));
        }
    }

    // Case B: legacy ~/.mostrix/settings.toml exists -> load with the old placeholder guard.
    if hidden_file.exists() {
        let settings = load_settings_from_path(&hidden_file)?;

        if settings.mostro_pubkey == "mostro_pubkey_hex_format"
            || settings.nsec_privkey == "nsec1_privkey_format"
        {
            let path_display = hidden_file.display();
            anyhow::bail!(
                "Default settings.toml already exists at {} but still contains placeholder values.\n\
Please edit this file and replace placeholder values (mostro_pubkey, nsec_privkey, etc.) \
with your real keys before running Mostrix again.",
                path_display
            );
        }

        return Ok((settings, false));
    }

    // Case C: Truly first run: no config anywhere.
    // Auto-generate in HOME/.mostrix with sensible defaults as per
    // https://github.com/MostroP2P/mostrix/issues/40.
    if !hidden_dir.exists() {
        fs::create_dir_all(&hidden_dir).map_err(|e| {
            anyhow::anyhow!("The configuration directory could not be created: {}", e)
        })?;
    }

    // Start from the embedded default template, then override fields.
    let mut settings: Settings = toml::from_str(DEFAULT_SETTINGS_TOML)
        .map_err(|e| anyhow::anyhow!("Embedded DEFAULT_SETTINGS_TOML is malformed: {}", e))?;

    // On first launch, derive the user `nsec` from the database identity/index-0 key
    // (generated from the mnemonic stored in `users`), so DB keys and settings match.
    let nsec = if let Some(identity) = identity_keys {
        let sk = identity.secret_key();
        sk.to_bech32()
            .map_err(|e| anyhow::anyhow!("Failed to encode identity Nostr secret key: {}", e))?
    } else {
        // Fallback: preserve older behavior if identity keys aren't provided.
        let keys = Keys::generate();
        let sk = keys.secret_key();
        sk.to_bech32()
            .map_err(|e| anyhow::anyhow!("Failed to encode generated Nostr secret key: {}", e))?
    };

    // Apply sensible defaults from the issue.
    settings.nsec_privkey = nsec;
    settings.relays = vec!["wss://relay.mostro.network".to_string()];
    settings.user_mode = "user".to_string();
    settings.currencies_filter = Vec::new();
    settings.mostro_pubkey = MOSTRO_STAGING_PUBKEY.to_string();

    // Serialize to TOML.
    let toml_string = toml::to_string_pretty(&settings)
        .map_err(|e| anyhow::anyhow!("Failed to serialize generated settings: {}", e))?;

    #[cfg(unix)]
    {
        use std::io::ErrorKind;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&hidden_file);

        match file {
            Ok(mut file) => {
                file.write_all(toml_string.as_bytes()).map_err(|e| {
                    anyhow::anyhow!("Could not write generated settings.toml: {}", e)
                })?;
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                // Another process won the race and created the file.
                // Load the persisted settings and apply the placeholder guard.
                let settings = load_settings_from_path(&hidden_file)?;
                if settings.mostro_pubkey == "mostro_pubkey_hex_format"
                    || settings.nsec_privkey == "nsec1_privkey_format"
                {
                    let path_display = hidden_file.display();
                    anyhow::bail!(
                        "Default settings.toml already exists at {} but still contains placeholder values.\n\
Please edit this file and replace placeholder values (mostro_pubkey, nsec_privkey, etc.) \
with your real keys before running Mostrix again.",
                        path_display
                    );
                }
                return Ok((settings, false));
            }
            Err(e) => {
                anyhow::bail!("Could not write generated settings.toml: {}", e);
            }
        }
    }

    #[cfg(not(unix))]
    {
        use std::io::ErrorKind;
        use std::io::Write;
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&hidden_file);

        match file {
            Ok(mut file) => {
                file.write_all(toml_string.as_bytes()).map_err(|e| {
                    anyhow::anyhow!("Could not write generated settings.toml: {}", e)
                })?;
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                // Another process won the race and created the file.
                // Load the persisted settings and apply the placeholder guard.
                let settings = load_settings_from_path(&hidden_file)?;
                if settings.mostro_pubkey == "mostro_pubkey_hex_format"
                    || settings.nsec_privkey == "nsec1_privkey_format"
                {
                    let path_display = hidden_file.display();
                    anyhow::bail!(
                        "Default settings.toml already exists at {} but still contains placeholder values.\n\
Please edit this file and replace placeholder values (mostro_pubkey, nsec_privkey, etc.) \
with your real keys before running Mostrix again.",
                        path_display
                    );
                }
                return Ok((settings, false));
            }
            Err(e) => {
                anyhow::bail!("Could not write generated settings.toml: {}", e);
            }
        }
    }

    Ok((settings, true))
}

/// Public helper: reload current settings from disk (reflects all previous saves)
pub fn load_settings_from_disk() -> Result<Settings, anyhow::Error> {
    Ok(init_or_load_settings_from_disk(None)?.0)
}

/// Save settings to file
pub fn save_settings(settings: &Settings) -> Result<(), anyhow::Error> {
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let package_name = env!("CARGO_PKG_NAME");
    let hidden_file_path = home_dir
        .join(format!(".{package_name}"))
        .join("settings.toml");
    let executable_file_path = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("settings.toml")));
    let target_settings_file = executable_file_path
        .filter(|p| p.exists())
        .unwrap_or(hidden_file_path);

    let toml_string = toml::to_string_pretty(settings)
        .map_err(|e| anyhow::anyhow!("Failed to serialize settings: {}", e))?;

    fs::write(&target_settings_file, toml_string)
        .map_err(|e| anyhow::anyhow!("Failed to write settings file: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_rejects_deprecated_currencies_field() {
        let toml = r#"
            mostro_pubkey = "npub1test"
            nsec_privkey = "nsec1test"
            admin_privkey = "nsec1admin"
            relays = ["wss://relay.example.com"]
            log_level = "info"
            currencies = ["USD", "EUR"]
        "#;
        // Direct deserialization (bypassing validate_currencies_config) should now fail
        // because the Settings struct no longer has a serde alias for `currencies`.
        let result: Result<Settings, _> = toml::from_str(toml);
        assert!(result.is_err());
    }
}
