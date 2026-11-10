//! krunk://scripts (0.12.0, WP-G): the lists, import/export, the editor's parse check
//! (`Runtime.compileScript` in the page's own tab) and hotkey recording.

use super::Browser;
use lb_core::protocol::{Event, Request};
use lb_core::tabs::TabId;

impl Browser {
    pub(super) fn scripts_request(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        let _ = request;
        self.reply(tab, Event::err(req_id, "Not available yet")); // WP-G
    }
}
