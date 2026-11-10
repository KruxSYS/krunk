//! Macros (0.12.0, WP-F): the hidden runner view, its API calls, the one running macro, hotkeys
//! (caught in tab views before the page), Stop and the toolbar indicator.

// 0.12.0 spine: parts of this are used once the work packages are wired in (M4).
#![allow(dead_code)]

use super::Browser;
use crate::ffi::cef::KeyPress;
use lb_core::agent::ScriptCard;
use lb_core::protocol::Request;
use lb_core::tabs::TabId;
use lb_core::windows::WindowId;

#[derive(Default)]
pub(super) struct MacroHost {}

impl Browser {
    /// The runner document loaded.
    pub(super) fn macro_runner_ready(&self) {
        // WP-F
    }

    /// `macroCall` / `macroDone` from the runner.
    pub(super) fn macro_runner_request(&self, req_id: Option<u32>, request: Request) {
        let _ = (req_id, request); // WP-F
    }

    pub(super) fn macro_running(&self) -> bool {
        false // WP-F
    }

    /// Stops the running macro (if any) and releases every held key and button.
    pub(super) fn stop_macros(&self, why: &str) {
        let _ = why;
        self.input_release_all(); // WP-F
    }

    /// The agent stopped (or its window closed): a macro it started stops too, and every held
    /// key and button is released.
    pub(super) fn stop_agent_macros(&self) {
        self.input_release_all(); // WP-F
    }

    /// A key in tab `tab` before the page sees it: hotkey recording on krunk://scripts, a
    /// macro's hotkey (start, or stop when it runs). True = handled.
    pub(super) fn macro_key(&self, tab: TabId, key: KeyPress) -> bool {
        let _ = (tab, key);
        false // WP-F
    }

    pub(super) fn send_macro_running_to(&self, win: WindowId) {
        let _ = win; // WP-F
    }

    pub(super) fn macro_tab_gone(&self, tab: TabId) {
        let _ = tab; // WP-F
    }

    pub(super) fn macros_settings_changed(&self) {
        // WP-F
    }

    /// Creates or updates a macro (`id` None = new). Checks the hotkey and the syntax.
    pub(super) fn macro_save(
        &self,
        id: Option<&str>,
        name: &str,
        source: &str,
        hotkey: Option<&str>,
        by_ai: bool,
    ) -> Result<ScriptCard, String> {
        let _ = (id, name, source, hotkey, by_ai);
        Err("not implemented (WP-F)".into())
    }

    /// Runs macro `id`; `done` gets its end (Ok with its result text, Err with its error) or,
    /// when `early_ms` passes first, Ok("running").
    pub(super) fn macro_run(
        &self,
        id: &str,
        by_agent: bool,
        early_ms: Option<u64>,
        done: impl FnOnce(Result<String, String>) + 'static,
    ) {
        let _ = (id, by_agent, early_ms);
        done(Err("not implemented (WP-F)".into()));
    }

    /// The macros for the agent's `list_macros`.
    pub(super) fn macros_list_json(&self) -> serde_json::Value {
        serde_json::json!([])
    }
}
