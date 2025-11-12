use crate::{SETTINGS, CONTEXT};
use nostr_sdk::prelude::*;
use sqlx::sqlite::SqlitePool;
use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize, Default)]
pub struct Settings {
    pub mostro_pubkey: String,
    pub identity_keys: String,
    pub trade_keys: String,
    pub trade_index: i64,
    pub context_keys: String,
    pub relays: Vec<String>,
    pub log_level: String,
    pub currencies: Vec<String>,
    pub pow: u8,
}


pub fn init_context() -> &'static Context{
     // Configure Nostr client.
    let my_keys = Keys::generate();
    let a = User::
    let client = Client::new(my_keys);
    // Add q.
    for relay in &settings.relays {
        client.add_relay(relay).await?;
    }
    client.connect().await;

    let mostro_pubkey = PublicKey::from_str(&settings.mostro_pubkey)
        .map_err(|e| anyhow::anyhow!("Invalid Mostro pubkey: {}", e))?;
    CONTEXT.get_or_init(|| {
        let client = Client::new(init_settings().identity_keys);
        let trade_keys = init_settings().trade_keys;
        let trade_index = init_settings().trade_index;
        let pool = init_db().await?;
        let context_keys = init_settings().context_keys;
        let mostro_pubkey = init_settings().mostro_pubkey;
    })
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
