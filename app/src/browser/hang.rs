//! A page that stops responding (0.11.0). Pages run in their own processes, so a hung page
//! never blocks the rest of Krunk, but there was no way to tell or to end it. After about
//! 15 s without handling input the engine reports it: while that tab is the active one, a
//! prompt says "This page isn't responding" with Wait (the engine asks again later) and Close
//! page (its renderer process ends). A page whose renderer ended, by Close page or by a crash,
//! shows Krunk's crash page with Reload, and keeps its address in the bar.
//!
//! Tabs of one site can share a renderer process: each of them gets the prompt, and Close page
//! ends all of them.

use super::{Browser, TabId};
use crate::log::{info, warning};
use lb_core::pages;
use lb_core::protocol::PopupContent;
use lb_core::urlclean;

impl Browser {
    /// The engine says tab `id`'s page stopped (`true`) or started again responding.
    pub(super) fn tab_unresponsive(&self, id: TabId, hung: bool) {
        let changed = self.views.borrow().get(&id).is_some_and(|v| v.unresponsive.replace(hung) != hung);
        if !changed {
            return;
        }
        if hung {
            warning!("tab {id}: the page is not responding");
            self.show_hang_prompt(id);
        } else {
            info!("tab {id}: the page responds again");
            self.hide_hang_prompt(id);
        }
    }

    /// Tab `id` became the active one (or hung): the prompt shows if its page does not respond.
    pub(super) fn show_hang_prompt(&self, id: TabId) {
        let hung = self.views.borrow().get(&id).is_some_and(|v| v.unresponsive.get());
        if !hung || self.active_id() != Some(id) {
            return;
        }
        let host = self.tab(id).and_then(|t| urlclean::host_of(&t.url)).unwrap_or_default();
        self.show_prompt(PopupContent::Unresponsive { tab_id: id, host }, 0);
    }

    fn hide_hang_prompt(&self, id: TabId) {
        let showing = self.popup.borrow().as_ref().is_some_and(|p| {
            p.open() && matches!(p.content, Some(PopupContent::Unresponsive { tab_id, .. }) if tab_id == id)
        });
        if showing {
            self.hide_popup();
        }
    }

    /// Wait or Close page.
    pub(super) fn hang_reply(&self, id: TabId, wait: bool) {
        self.hide_hang_prompt(id);
        let Some(view) = self.tab_view(id) else { return };
        if wait {
            info!("tab {id}: waiting for the page");
            // The engine asks again if it still does not respond.
            if let Some(v) = self.views.borrow().get(&id) {
                v.unresponsive.set(false);
            }
            view.hang_wait();
        } else {
            info!("tab {id}: the page is closed (renderer ended)");
            view.hang_terminate();
        }
    }

    /// Tab `id` navigates: an old prompt for it is gone.
    pub(super) fn hang_navigated(&self, id: TabId) {
        let was = self.views.borrow().get(&id).is_some_and(|v| v.unresponsive.replace(false));
        if was {
            self.hide_hang_prompt(id);
        }
    }

    /// The renderer of tab `id` ended (crash, or Close page): the crash page with Reload.
    pub(super) fn show_crashed_page(&self, id: TabId) {
        self.hang_navigated(id);
        let Some(url) = self.tab(id).map(|t| t.url.clone()) else { return };
        if pages::Page::from_url(&url) == Some(pages::Page::Crashed) {
            return;
        }
        if url.starts_with("http://") || url.starts_with("https://") {
            self.navigate_tab(id, &pages::crashed_url(&url));
        } else if pages::Page::from_url(&url).is_some() {
            // One of Krunk's own pages: load it again.
            self.navigate_tab(id, &url);
        }
    }
}
