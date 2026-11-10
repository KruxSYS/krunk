//! Userscripts (0.12.0, WP-E): one loader per tab, registered as a new-document script in the
//! main world, re-registered when scripts or values change; GM calls through a hidden binding.

// 0.12.0 spine: parts of this are used once the work packages are wired in (M4).
#![allow(dead_code)]

use super::Browser;
use crate::ffi::cef::View;
use lb_core::agent::ScriptCard;
use lb_core::tabs::TabId;

#[derive(Default)]
pub(super) struct UserscriptHost {}

impl Browser {
    pub(super) fn userscripts_attach_tab(&self, tab: TabId, view: &View) {
        let _ = (tab, view); // WP-E
    }

    /// The tab's browser exists: register its loader.
    pub(super) fn userscripts_tab_created(&self, tab: TabId) {
        let _ = tab; // WP-E
    }

    pub(super) fn userscripts_detach_tab(&self, tab: TabId) {
        let _ = tab; // WP-E
    }

    pub(super) fn userscripts_settings_changed(&self) {
        // WP-E
    }

    /// Creates or updates a userscript from its source (`id` None = new; a new one is enabled).
    pub(super) fn userscript_save(&self, id: Option<&str>, source: &str, by_ai: bool) -> Result<ScriptCard, String> {
        let _ = (id, source, by_ai);
        Err("not implemented (WP-E)".into())
    }

    pub(super) fn userscript_toggle(&self, id: &str, enabled: bool) -> Result<ScriptCard, String> {
        let _ = (id, enabled);
        Err("not implemented (WP-E)".into())
    }

    /// Runs userscript `id` once in tab `tab` now (if it matches the tab's address unless
    /// `force`). Ok(true) = it ran.
    pub(super) fn userscript_run_now(&self, id: &str, tab: TabId, force: bool) -> Result<bool, String> {
        let _ = (id, tab, force);
        Err("not implemented (WP-E)".into())
    }

    /// The userscripts for the agent's `list_userscripts`.
    pub(super) fn userscripts_list_json(&self) -> serde_json::Value {
        serde_json::json!([])
    }
}
