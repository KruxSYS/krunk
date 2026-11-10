//! Settings → AI (0.12.0, WP-A): providers, keys, the key test and model lists.

use super::Browser;
use lb_core::protocol::{Event, Request};
use lb_core::tabs::TabId;

impl Browser {
    pub(super) fn ai_settings_request(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        let _ = request;
        self.reply(tab, Event::err(req_id, "Not available yet")); // WP-A
    }
}
