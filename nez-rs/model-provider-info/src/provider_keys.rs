//! Provider API keys kept in the operating system's credential store
//! (Windows Credential Manager, macOS Keychain, Secret Service on Linux).
//!
//! Keys are stored per environment variable name (`DEEPSEEK_API_KEY`), so a
//! stored key behaves exactly like the variable being set. The variable still
//! wins when both exist. Keys are never written to `config.toml`.

use nez_keyring_store::DefaultKeyringStore;
use nez_keyring_store::KeyringStore;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;

const SERVICE: &str = "Nez-Code";

fn account(env_key: &str) -> String {
    format!("provider-api-key:{env_key}")
}

/// Lookups hit the credential store at most once per process; requests read
/// the key on every turn.
fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The stored key for `env_key`, if any. Credential store errors (for example
/// no Secret Service on a headless Linux box) count as "no key".
pub fn load_provider_key(env_key: &str) -> Option<String> {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache
        .entry(env_key.to_string())
        .or_insert_with(|| load_with(&DefaultKeyringStore, env_key))
        .clone()
}

pub fn save_provider_key(env_key: &str, key: &str) -> Result<(), String> {
    save_with(&DefaultKeyringStore, env_key, key)?;
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(env_key.to_string(), Some(key.to_string()));
    Ok(())
}

/// Returns whether a key was removed.
pub fn delete_provider_key(env_key: &str) -> Result<bool, String> {
    let removed = delete_with(&DefaultKeyringStore, env_key)?;
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(env_key.to_string(), None);
    Ok(removed)
}

pub(crate) fn load_with(store: &dyn KeyringStore, env_key: &str) -> Option<String> {
    match store.load(SERVICE, &account(env_key)) {
        Ok(key) => key.filter(|k| !k.trim().is_empty()),
        Err(err) => {
            tracing::debug!(
                "credential store unavailable for {env_key}: {}",
                err.message()
            );
            None
        }
    }
}

pub(crate) fn save_with(store: &dyn KeyringStore, env_key: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("the API key is empty".to_string());
    }
    store.save(SERVICE, &account(env_key), key).map_err(|err| {
        format!(
            "could not save the key to the system credential store ({}). Set {env_key} in your environment instead.",
            err.message()
        )
    })
}

pub(crate) fn delete_with(store: &dyn KeyringStore, env_key: &str) -> Result<bool, String> {
    store
        .delete(SERVICE, &account(env_key))
        .map_err(|err| format!("could not remove the stored key: {}", err.message()))
}

#[cfg(test)]
#[path = "provider_keys_tests.rs"]
mod tests;
