use crate::ui::key_handler::validation::normalize_blossom_server_url;
use crate::ui::{AppState, UserRole};
use crate::util::blossom::default_blossom_servers;
use lnurl::lightning_address::LightningAddress;
use nostr_sdk::prelude::RelayUrl;
use std::str::FromStr;

/// Result-returning core behind the settings helpers: load the latest on-disk
/// state, apply `update_fn`, persist. `Ok(())` is returned only after a
/// successful disk write, so callers can gate side effects (e.g. touching the
/// running Nostr client) on persistence.
fn try_update_settings<F>(update_fn: F, error_msg: &str, success_msg: &str) -> Result<(), String>
where
    F: FnOnce(&mut crate::settings::Settings) -> Result<(), String>,
{
    let mut current_settings = crate::settings::load_settings_from_disk().map_err(|e| {
        log::error!("Failed to load settings for update: {}", e);
        format!("Failed to load settings for update: {e}")
    })?;
    // Apply the caller's mutation on top of the latest on-disk state
    update_fn(&mut current_settings)?;
    crate::settings::save_settings(&current_settings).map_err(|e| {
        log::error!("{}: {}", error_msg, e);
        format!("{error_msg}: {e}")
    })?;
    log::info!("{}", success_msg);
    Ok(())
}

/// Infallible-mutation variant of [`try_update_settings`].
fn try_save_settings_with<F>(update_fn: F, error_msg: &str, success_msg: &str) -> Result<(), String>
where
    F: FnOnce(&mut crate::settings::Settings),
{
    try_update_settings(
        |s| {
            update_fn(s);
            Ok(())
        },
        error_msg,
        success_msg,
    )
}

/// Generic helper to save settings with a custom update function.
/// Load/save failures are logged; callers that need to react should use a
/// `Result`-returning helper instead (e.g. [`remove_relay_from_settings`]).
pub fn save_settings_with<F>(update_fn: F, error_msg: &str, success_msg: &str)
where
    F: FnOnce(&mut crate::settings::Settings),
{
    let _ = try_save_settings_with(update_fn, error_msg, success_msg);
}

/// Save admin key to settings file; returns `Ok(())` only after a successful disk write.
pub fn try_save_admin_key_to_settings(key_string: &str) -> Result<(), String> {
    match crate::settings::load_settings_from_disk() {
        Ok(mut current_settings) => {
            current_settings.admin_privkey = key_string.to_string();
            crate::settings::save_settings(&current_settings)
                .map_err(|e| format!("Failed to save admin key to settings: {e}"))
        }
        Err(e) => Err(format!("Failed to load settings for update: {e}")),
    }
}

/// Save Mostro pubkey to settings file; `Ok(())` only after a successful disk write.
///
/// `key_string` should already be lowercase hex from [`super::validation::normalize_mostro_pubkey`].
pub fn save_mostro_pubkey_to_settings(key_string: &str) -> Result<(), String> {
    try_save_settings_with(
        |s| s.mostro_pubkey = key_string.to_string(),
        "Failed to save Mostro pubkey to settings",
        "Mostro pubkey saved to settings file",
    )
}

/// Validate Lightning address shape (`user@domain.com`) before opening the confirm dialog.
/// Saving runs an async LNURL metadata check (`tag: payRequest`) before writing disk.
pub fn validate_ln_address_format(addr: &str) -> Result<(), String> {
    let t = addr.trim();
    if t.is_empty() {
        return Err("Lightning address cannot be empty".to_string());
    }
    LightningAddress::from_str(t)
        .map(|_| ())
        .map_err(|_| "Invalid Lightning address (expected user@domain.com)".to_string())
}

pub fn clear_ln_address_from_settings() {
    save_settings_with(
        |s| s.ln_address.clear(),
        "Failed to clear Lightning address",
        "Lightning address cleared from settings file",
    );
}

/// Save relay to settings file; `Ok(())` only after a successful disk write.
pub fn save_relay_to_settings(relay_string: &str) -> Result<(), String> {
    try_save_settings_with(
        |s| {
            if !s.relays.contains(&relay_string.to_string()) {
                s.relays.push(relay_string.to_string());
            }
        },
        "Failed to save relay to settings",
        "Relay added to settings file",
    )
}

/// Pure mutation behind [`remove_relay_from_settings`]: deletes only the first
/// matching entry (hand-edited duplicates survive) and refuses to empty the
/// list, so the last-relay guard holds even if the UI state is stale.
fn remove_relay_entry(relays: &mut Vec<String>, relay_string: &str) -> Result<(), String> {
    let Some(pos) = relays.iter().position(|r| r == relay_string) else {
        // Not present on disk: nothing to persist.
        return Ok(());
    };
    if relays.len() <= 1 {
        return Err("At least one relay is required; cannot remove the last relay.".to_string());
    }
    relays.remove(pos);
    Ok(())
}

/// Remove a relay from the settings file; `Ok(())` only after a successful disk write.
pub fn remove_relay_from_settings(relay_string: &str) -> Result<(), String> {
    try_update_settings(
        |s| remove_relay_entry(&mut s.relays, relay_string),
        "Failed to remove relay from settings",
        "Relay removed from settings file",
    )
}

/// Replace the relay list with the built-in defaults; `Ok(())` only after a
/// successful disk write.
pub fn restore_default_relays_in_settings() -> Result<(), String> {
    try_save_settings_with(
        |s| s.relays = crate::settings::default_relays(),
        "Failed to restore default relays",
        "Default relays restored in settings file",
    )
}

fn same_blossom_server(a: &str, b: &str) -> bool {
    normalize_blossom_server_url(a).eq_ignore_ascii_case(&normalize_blossom_server_url(b))
}

fn ensure_blossom_servers_materialized(servers: &mut Vec<String>) {
    if servers.is_empty() {
        *servers = default_blossom_servers();
    }
}

/// Append a Blossom server. An empty on-disk list means "use defaults", so the
/// first edit materializes those defaults before inserting — otherwise adding
/// one host would replace the whole built-in set.
fn add_blossom_entry(servers: &mut Vec<String>, server: &str) {
    ensure_blossom_servers_materialized(servers);
    if !servers.iter().any(|s| same_blossom_server(s, server)) {
        servers.push(server.to_string());
    }
}

/// Pure mutation behind [`remove_blossom_server_from_settings`]: materializes
/// defaults when the list is empty, then refuses to delete the last server.
fn remove_blossom_entry(servers: &mut Vec<String>, server: &str) -> Result<(), String> {
    ensure_blossom_servers_materialized(servers);
    let Some(pos) = servers.iter().position(|s| same_blossom_server(s, server)) else {
        return Ok(());
    };
    if servers.len() <= 1 {
        return Err(
            "At least one Blossom server is required; cannot remove the last server.".to_string(),
        );
    }
    servers.remove(pos);
    Ok(())
}

/// Effective Blossom list for the remove picker (empty on disk → built-in defaults).
pub fn load_blossom_servers_for_ui() -> Vec<String> {
    crate::settings::load_settings_from_disk()
        .map(|s| crate::util::send_attachment::blossom_servers_from_settings(&s))
        .unwrap_or_else(|_| default_blossom_servers())
}

/// Save a Blossom server to settings; `Ok(())` only after a successful disk write.
///
/// An empty on-disk list is treated as "use defaults": those hosts are written
/// first, then `server` is appended unless it is already in the list.
pub fn save_blossom_server_to_settings(server: &str) -> Result<(), String> {
    try_save_settings_with(
        |s| add_blossom_entry(&mut s.blossom_servers, server),
        "Failed to save Blossom server to settings",
        "Blossom server added to settings file",
    )
}

/// Remove a Blossom server from settings; `Ok(())` only after a successful disk write.
///
/// An empty on-disk list is materialized to the built-in defaults first. Refuses
/// to delete the last remaining server.
pub fn remove_blossom_server_from_settings(server: &str) -> Result<(), String> {
    try_update_settings(
        |s| remove_blossom_entry(&mut s.blossom_servers, server),
        "Failed to remove Blossom server from settings",
        "Blossom server removed from settings file",
    )
}

/// Replace the Blossom list with [`crate::util::blossom::default_blossom_servers`];
/// `Ok(())` only after a successful disk write.
pub fn restore_default_blossom_servers_in_settings() -> Result<(), String> {
    try_save_settings_with(
        |s| s.blossom_servers = default_blossom_servers(),
        "Failed to restore default Blossom servers",
        "Default Blossom servers restored in settings file",
    )
}

/// Two relay strings name the same relay when their parsed [`RelayUrl`]s are
/// equal (the SDK keys its pool by parsed URL, so `wss://host` and
/// `wss://host/` — or different host casing — are one relay). Unparseable
/// values fall back to trimmed string equality.
fn same_relay(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    match (RelayUrl::parse(a), RelayUrl::parse(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Diff two relay lists the way the running client sees them: which URLs to
/// add to / remove from the pool when replacing `old_relays` with
/// `new_relays`. Comparing by parsed [`RelayUrl`] avoids removing a pool entry
/// that the new list keeps under an equivalent spelling.
pub fn plan_relay_reconcile(
    old_relays: &[String],
    new_relays: &[String],
) -> (Vec<String>, Vec<String>) {
    let to_add = new_relays
        .iter()
        .filter(|n| !old_relays.iter().any(|o| same_relay(o, n)))
        .cloned()
        .collect();
    let to_remove = old_relays
        .iter()
        .filter(|o| !new_relays.iter().any(|n| same_relay(o, n)))
        .cloned()
        .collect();
    (to_add, to_remove)
}

/// Save currency to settings file
pub fn save_currency_to_settings(currency_string: &str) {
    save_settings_with(
        |s| {
            let currency_upper = currency_string.trim().to_uppercase();
            if !s.currencies_filter.contains(&currency_upper) {
                s.currencies_filter.push(currency_upper);
            }
        },
        "Failed to save currency to settings",
        "Currency filter added to settings file",
    );
}

/// Clear all currency filters (sets currencies to empty vector)
pub fn clear_currency_filters() {
    save_settings_with(
        |s| {
            s.currencies_filter.clear();
        },
        "Failed to clear currency filters",
        "All currency filters cleared",
    );
}

/// Toggle User/Admin from Settings (Enter on "Switch Mode").
pub fn handle_mode_switch(app: &mut AppState) {
    let new_role = match app.user_role {
        UserRole::User => UserRole::Admin,
        UserRole::Admin => UserRole::User,
    };

    app.switch_role(new_role);

    if new_role == UserRole::Admin {
        app.pending_admin_disputes_reload = true;
    }

    let role_string = new_role.to_string();
    save_settings_with(
        |s| s.user_mode = role_string.clone(),
        "Failed to switch mode in settings",
        &format!("Mode switched to: {}", new_role),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_relay_entry_removes_only_first_match() {
        let mut relays = vec![
            "wss://relay.one".to_string(),
            "wss://relay.one".to_string(),
            "wss://relay.two".to_string(),
        ];
        remove_relay_entry(&mut relays, "wss://relay.one").expect("removal must succeed");
        assert_eq!(
            relays,
            vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
            "duplicate entries must survive a single removal"
        );
    }

    #[test]
    fn remove_relay_entry_refuses_to_empty_list() {
        let mut relays = vec!["wss://relay.one".to_string()];
        let err = remove_relay_entry(&mut relays, "wss://relay.one")
            .expect_err("removing the last relay must fail");
        assert!(err.contains("At least one relay"));
        assert_eq!(relays, vec!["wss://relay.one".to_string()]);
    }

    #[test]
    fn remove_relay_entry_absent_relay_is_a_noop() {
        let mut relays = vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()];
        remove_relay_entry(&mut relays, "wss://relay.unknown").expect("no-op must succeed");
        assert_eq!(relays.len(), 2);
    }

    #[test]
    fn reconcile_treats_trailing_slash_as_same_relay() {
        // Regression: a pooled relay saved with a trailing slash must not be
        // removed when the slash-less default replaces it on disk.
        let old = vec!["wss://relay.mostro.network/".to_string()];
        let new = vec![
            "wss://relay.mostro.network".to_string(),
            "wss://relay.shadowbip.com".to_string(),
        ];
        let (to_add, to_remove) = plan_relay_reconcile(&old, &new);
        assert_eq!(to_add, vec!["wss://relay.shadowbip.com".to_string()]);
        assert!(
            to_remove.is_empty(),
            "equivalent default must stay in the pool"
        );
    }

    #[test]
    fn reconcile_treats_host_case_as_same_relay() {
        let old = vec!["wss://RELAY.MOSTRO.NETWORK".to_string()];
        let new = vec!["wss://relay.mostro.network".to_string()];
        let (to_add, to_remove) = plan_relay_reconcile(&old, &new);
        assert!(to_add.is_empty());
        assert!(to_remove.is_empty());
    }

    #[test]
    fn reconcile_diffs_plain_entries() {
        let old = vec![
            "wss://relay.custom".to_string(),
            "wss://relay.mostro.network".to_string(),
        ];
        let new = vec!["wss://relay.mostro.network".to_string()];
        let (to_add, to_remove) = plan_relay_reconcile(&old, &new);
        assert!(to_add.is_empty());
        assert_eq!(to_remove, vec!["wss://relay.custom".to_string()]);
    }

    #[test]
    fn reconcile_falls_back_to_string_equality_for_unparseable() {
        let old = vec!["not a url".to_string()];
        let new = vec!["wss://relay.mostro.network".to_string()];
        let (to_add, to_remove) = plan_relay_reconcile(&old, &new);
        assert_eq!(to_add, vec!["wss://relay.mostro.network".to_string()]);
        assert_eq!(to_remove, vec!["not a url".to_string()]);
    }

    #[test]
    fn add_blossom_entry_materializes_defaults_before_insert() {
        let mut servers = Vec::new();
        add_blossom_entry(&mut servers, "https://custom.example");
        let defaults = default_blossom_servers();
        assert_eq!(servers.len(), defaults.len() + 1);
        assert_eq!(servers[defaults.len()], "https://custom.example");
        assert_eq!(&servers[..defaults.len()], defaults.as_slice());
    }

    #[test]
    fn add_blossom_entry_skips_duplicate_default() {
        let mut servers = Vec::new();
        let first = default_blossom_servers()[0].clone();
        add_blossom_entry(&mut servers, &format!("{first}/"));
        assert_eq!(servers, default_blossom_servers());
    }

    #[test]
    fn remove_blossom_entry_refuses_to_empty_list() {
        let mut servers = vec!["https://only.example".to_string()];
        let err = remove_blossom_entry(&mut servers, "https://only.example")
            .expect_err("removing the last blossom server must fail");
        assert!(err.contains("At least one Blossom server"));
        assert_eq!(servers, vec!["https://only.example".to_string()]);
    }

    #[test]
    fn remove_blossom_entry_from_empty_materializes_then_removes() {
        let mut servers = Vec::new();
        let first = default_blossom_servers()[0].clone();
        remove_blossom_entry(&mut servers, &first).expect("removal must succeed");
        let mut expected = default_blossom_servers();
        expected.remove(0);
        assert_eq!(servers, expected);
    }
}
