//! The AI providers' API keys (0.12.0, WP-A): DPAPI-protected (current user, Krunk's own
//! entropy) in the `ai_keys` table. A key is unwrapped into zeroizing memory only for the
//! request that needs it, and never sent to any view, logged, synced or exported.

// 0.12.0 spine: parts of this are used once the work packages are wired in (M4).
#![allow(dead_code)]

use super::Browser;

impl Browser {
    /// Stores provider `id`'s key (replacing an older one).
    pub(super) fn ai_key_set(&self, id: &str, key: &str) -> Result<(), String> {
        let _ = (id, key);
        Err("not implemented (WP-A)".into())
    }

    pub(super) fn ai_key_remove(&self, id: &str) {
        let _ = self.store.borrow().delete_ai_key(id);
    }

    pub(super) fn ai_key_has(&self, id: &str) -> bool {
        self.store.borrow().ai_key_blob(id).ok().flatten().is_some()
    }

    /// Runs `f` with provider `id`'s key (empty when it has none); the key is wiped after.
    pub(super) fn with_ai_key<R>(&self, id: &str, f: impl FnOnce(&str) -> R) -> Result<R, String> {
        let _ = id;
        Ok(f(""))
    }
}
