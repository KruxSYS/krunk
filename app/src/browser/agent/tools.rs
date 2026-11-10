//! The agent's tools on the host side (0.12.0, WP-B2): each one runs against the window's
//! target tab (CDP, the probe in its isolated world, engine input), the browser's tabs,
//! settings, downloads, userscripts and macros, and returns a redacted, capped `ToolOutcome`.

use super::super::Browser;
use lb_core::agent::ToolOutcome;
use lb_core::windows::WindowId;
use serde_json::Value;

impl Browser {
    /// Runs tool `name` with validated `args` for window `win`'s agent; `done` gets the outcome
    /// (exactly once, also on failure).
    pub(super) fn run_agent_tool(&self, win: WindowId, name: &str, args: Value, done: Box<dyn FnOnce(ToolOutcome)>) {
        let _ = (win, args);
        done(ToolOutcome::error(format!("{name} is not implemented yet")));
    }
}
