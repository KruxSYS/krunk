//! The agent's tools on the host side (0.12.0, WP-B2): each one runs against the window's
//! target tab (CDP, the probe in its isolated world, engine input), the browser's tabs,
//! settings, downloads, userscripts and macros, and returns a redacted, capped `ToolOutcome`.
//!
//! Interim after the lost session: `agent_target` is real, the probe and the tools are not yet
//! (the relaunched WP-B2 package fills them in).

use super::super::Browser;
use lb_core::agent::ToolOutcome;
use lb_core::tabs::TabId;
use lb_core::windows::WindowId;
use serde_json::Value;

/// The tab the agent of a window acts on: the window's active tab, at its committed address.
pub(super) struct Target {
    pub tab: TabId,
    pub url: String,
}

impl Browser {
    /// Window `win`'s target tab (its active tab).
    pub(super) fn agent_target(&self, win: WindowId) -> Option<Target> {
        let tab = self.ws.borrow().model(win)?.active_id()?;
        let url = self.tab(tab)?.url.clone();
        Some(Target { tab, url })
    }

    /// Calls the probe's function `func` with `args` (a JSON array) in tab `tab`'s isolated
    /// world; `done` gets its JSON result.
    pub(super) fn agent_probe(
        &self,
        tab: TabId,
        func: &str,
        args: Value,
        done: impl FnOnce(Result<Value, String>) + 'static,
    ) {
        let _ = (tab, func, args);
        done(Err("the page probe is not available yet".into()));
    }

    /// Runs tool `name` with validated `args` for window `win`'s agent; `done` gets the outcome
    /// (exactly once, also on failure).
    pub(super) fn run_agent_tool(&self, win: WindowId, name: &str, args: Value, done: Box<dyn FnOnce(ToolOutcome)>) {
        let _ = (win, args);
        done(ToolOutcome::error(format!("{name} is not implemented yet")));
    }
}
