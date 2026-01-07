use crate::ui::{AppState, UserRole};
use crate::SETTINGS;

/// Generic helper to save settings with a custom update function
pub fn save_settings_with<F>(update_fn: F, error_msg: &str, success_msg: &str)
where
    F: FnOnce(&mut crate::settings::Settings),
{
    if let Some(settings) = SETTINGS.get() {
        let mut new_settings = settings.clone();
        update_fn(&mut new_settings);
        if let Err(e) = crate::settings::save_settings(&new_settings) {
            log::error!("{}: {}", error_msg, e);
        } else {
            log::info!("{}", success_msg);
        }
    }
}

/// Save admin key to settings file
pub fn save_admin_key_to_settings(key_string: &str) {
    save_settings_with(
        |s| s.admin_privkey = key_string.to_string(),
        "Failed to save admin key to settings",
        "Admin key saved to settings file",
    );
}

/// Save Mostro pubkey to settings file
pub fn save_mostro_pubkey_to_settings(key_string: &str) {
    save_settings_with(
        |s| s.mostro_pubkey = key_string.to_string(),
        "Failed to save Mostro pubkey to settings",
        "Mostro pubkey saved to settings file",
    );
}

/// Save relay to settings file
pub fn save_relay_to_settings(relay_string: &str) {
    save_settings_with(
        |s| {
            if !s.relays.contains(&relay_string.to_string()) {
                s.relays.push(relay_string.to_string());
            }
        },
        "Failed to save relay to settings",
        "Relay added to settings file",
    );
}

/// Handle mode switching (M key in Settings tab)
pub fn handle_mode_switch(app: &mut AppState, _settings: &crate::settings::Settings) {
    let new_role = match app.user_role {
        UserRole::User => UserRole::Admin,
        UserRole::Admin => UserRole::User,
    };

    // Update app state
    app.switch_role(new_role);

    // Save to settings file
    let role_string = new_role.to_string();
    save_settings_with(
        |s| s.user_mode = role_string.clone(),
        "Failed to switch mode in settings",
        &format!("Mode switched to: {}", new_role),
    );
}
