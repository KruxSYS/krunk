//! The AI providers' API keys (0.12.0, WP-A): DPAPI-protected (current user, Krunk's own
//! entropy) in the `ai_keys` table. A key is unwrapped into zeroizing memory only for the
//! request that needs it, and never sent to any view, logged, synced or exported.

use super::Browser;
use crate::ffi::secure::DpapiAgent;
use crate::log::warning;
use lb_core::vault::KeyWrapper;

/// The longest key accepted (real ones are under 200 characters).
const MAX_KEY_LEN: usize = 1000;

impl Browser {
    /// Stores provider `id`'s key (replacing an older one). The key is trimmed; it must be
    /// printable ASCII without spaces, as it goes into an HTTP header.
    pub(super) fn ai_key_set(&self, id: &str, key: &str) -> Result<(), String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("Enter a key".into());
        }
        if key.len() > MAX_KEY_LEN {
            return Err("This key is too long".into());
        }
        if !key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err("A key has only letters, digits and signs, without spaces".into());
        }
        let blob = DpapiAgent.protect(key.as_bytes()).map_err(|e| {
            warning!("ai key: protecting failed: {e}");
            "Could not protect the key".to_string()
        })?;
        self.store.borrow().set_ai_key_blob(id, &blob, lb_core::storage::now_ms()).map_err(|e| {
            warning!("ai key: saving failed: {e}");
            "Could not save the key".to_string()
        })?;
        self.provider_models_forget(id);
        Ok(())
    }

    pub(super) fn ai_key_remove(&self, id: &str) {
        if let Err(e) = self.store.borrow().delete_ai_key(id) {
            warning!("ai key: removing failed: {e}");
        }
        self.provider_models_forget(id);
    }

    /// For the agent host's configuration check ("add a key first"), wired in M4.
    #[allow(dead_code)]
    pub(super) fn ai_key_has(&self, id: &str) -> bool {
        self.store.borrow().ai_key_blob(id).ok().flatten().is_some()
    }

    /// Runs `f` with provider `id`'s key (empty when it has none); the key is wiped after.
    pub(super) fn with_ai_key<R>(&self, id: &str, f: impl FnOnce(&str) -> R) -> Result<R, String> {
        let blob = self.store.borrow().ai_key_blob(id).map_err(|e| {
            warning!("ai key: reading failed: {e}");
            "Could not read the stored key".to_string()
        })?;
        let Some(blob) = blob else { return Ok(f("")) };
        // Zeroizing: wiped when it goes out of scope, after `f`.
        let plain = DpapiAgent.unprotect(&blob).map_err(|e| {
            warning!("ai key: unprotecting failed: {e}");
            "The stored key could not be opened. Enter it again.".to_string()
        })?;
        let key = std::str::from_utf8(&plain).map_err(|_| "The stored key is damaged. Enter it again.".to_string())?;
        Ok(f(key))
    }
}
