//! The agent's tools on the host side (0.12.0, WP-B2): each one runs against the window's
//! target tab (CDP, the probe in its isolated world, engine input), the browser's tabs,
//! settings, downloads, userscripts and macros, and returns a redacted, capped `ToolOutcome`.
//!
//! Page tools (`Group::Page`) are in `page.rs` (WP-B2a); this file (WP-B2b) dispatches every
//! tool and runs the others: navigation and tabs, settings, userscripts, macros and the
//! sensitive-data tools.

use super::super::Browser;
use lb_core::agent::tools::{self as agent_tools, Group};
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

    /// Runs tool `name` with validated `args` for window `win`'s agent; `done` gets the outcome
    /// (exactly once, also on failure).
    pub(super) fn run_agent_tool(&self, win: WindowId, name: &str, args: Value, done: Box<dyn FnOnce(ToolOutcome)>) {
        let Some(tool) = agent_tools::find(name) else {
            done(ToolOutcome::error("there is no such tool"));
            return;
        };
        match tool.group {
            Group::Page => self.agent_page_tool(win, tool.name, args, done),
            _ => done(ToolOutcome::error(format!("{name} is not implemented yet"))), // WP-B2b
        }
    }
}
