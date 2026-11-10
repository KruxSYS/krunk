//! krunk://scripts (0.12.0, WP-G): the lists, import/export, the editor's parse check
//! (`Runtime.compileScript` in the page's own tab) and hotkey recording.

// 0.12.0 spine: parts of this are used once the work packages are wired in (M4).
#![allow(dead_code)]

use super::Browser;
use lb_core::protocol::{Event, Request};
use lb_core::tabs::TabId;

impl Browser {
    /// Tells every open krunk://scripts page that the lists changed (a save, toggle, delete, a
    /// script the agent made, a last-run time).
    pub(super) fn notify_scripts_changed(&self) {
        self.post_scripts_pages(&Event::ScriptsChanged);
    }

    /// Posts an event to every tab showing krunk://scripts.
    pub(super) fn post_scripts_pages(&self, event: &Event) {
        let _ = event; // WP-G
    }

    pub(super) fn scripts_request(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        let _ = request;
        self.reply(tab, Event::err(req_id, "Not available yet")); // WP-G
    }
}
