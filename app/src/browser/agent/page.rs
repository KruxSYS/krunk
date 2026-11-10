//! The agent's page tools (0.12.0, WP-B2a): the probe in an isolated world of the target tab
//! (`ui/inject/agent-probe.js`), screenshots, engine input and the page-editing tools — every
//! tool of `Group::Page` in `lb_core::agent::tools`. Results are redacted, scrubbed and capped
//! here before they go back.

use super::super::Browser;
use lb_core::agent::ToolOutcome;
use lb_core::tabs::TabId;
use lb_core::windows::WindowId;
use serde_json::Value;

impl Browser {
    /// Calls the probe's function `func` with `args` (a JSON array) in tab `tab`'s isolated
    /// world (created and the probe injected on first use per document); `done` gets its JSON
    /// result.
    pub(super) fn agent_probe(
        &self,
        tab: TabId,
        func: &str,
        args: Value,
        done: impl FnOnce(Result<Value, String>) + 'static,
    ) {
        let _ = (tab, func, args);
        done(Err("the page probe is not available yet".into())); // WP-B2a
    }

    /// Runs a page tool (`Group::Page`) for window `win`'s agent on its target tab. The caller
    /// has validated `args`; the built-in-page check (`pages::may_act`) is done here.
    pub(super) fn agent_page_tool(
        &self,
        win: WindowId,
        name: &'static str,
        args: Value,
        done: Box<dyn FnOnce(ToolOutcome)>,
    ) {
        let _ = (win, args);
        done(ToolOutcome::error(format!("{name} is not implemented yet"))); // WP-B2a
    }
}
