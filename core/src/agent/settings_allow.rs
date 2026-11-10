//! The browser settings the agent may read and change (WP-B1).
//!
//! An explicit allow-list of camelCase key paths. Everything about the vault, payment cards,
//! sync, the account, the AI providers and keys, and security-relevant network settings
//! (proxy, DNS, extra flags, user agent, headers, updates, the download folder, custom search
//! or mail templates) is not on it and cannot be read or set.

use crate::settings::Settings;
use serde_json::Value;

/// The allowed key paths (`theme`, `appearance.showZoomChip`, …).
pub fn allowed_keys() -> &'static [&'static str] {
    &[] // WP-B1
}

/// The allowed settings with their values and, for choices, the allowed values:
/// `{"theme": {"value": "system", "values": ["system","light","dark"]}, …}`.
pub fn get_all(s: &Settings) -> Value {
    let _ = s;
    Value::Object(Default::default()) // WP-B1
}

/// The result of a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// The change takes effect after a restart.
    pub restart: bool,
}

/// Changes one allowed setting; validates the whole result with [`Settings::validate`].
pub fn set(s: &mut Settings, key: &str, value: Value) -> Result<Changed, String> {
    let _ = (s, key, value);
    Err("not available".into()) // WP-B1
}
