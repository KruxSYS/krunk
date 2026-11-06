//! Local network access (Krunk 0.5.0). Chromium's Local Network Access check asks the embedder
//! before a public page may reach a private address or this computer; Krunk answers from the
//! user's stored choice for the site, or shows its own prompt ("Allow" / "Block", remembered
//! for the site unless the tab is private). A prompt for a tab that is not active waits until
//! the tab is; dismissing the prompt blocks once.

use std::cell::RefCell;

use lb_core::permissions::LOCAL_NETWORK;
use lb_core::protocol::PopupContent;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;

use super::Browser;
use crate::ffi::cef::View;
use crate::log::info;

/// A prompt the engine opened for a tab.
#[derive(Debug, Clone)]
struct Ask {
    tab: TabId,
    prompt_id: u64,
    origin: String,
    loopback: bool,
}

#[derive(Default)]
pub(super) struct PermissionHost {
    /// Prompts waiting for their tab to become active (or for the current prompt to close).
    waiting: RefCell<Vec<Ask>>,
    /// The prompt shown now.
    showing: RefCell<Option<Ask>>,
}

impl Browser {
    pub(super) fn hook_permissions(&self, id: TabId, c: &View) {
        let weak = self.me.clone();
        c.on_permission(move |prompt_id, origin, loopback| {
            if let Some(b) = weak.upgrade() {
                b.permission_requested(Ask { tab: id, prompt_id, origin, loopback });
            }
        });
        let weak = self.me.clone();
        c.on_permission_gone(move |prompt_id| {
            if let Some(b) = weak.upgrade() {
                b.permission_withdrawn(id, prompt_id);
            }
        });
    }

    fn permission_requested(&self, ask: Ask) {
        let private = self.tab(ask.tab).is_some_and(|t| t.private);
        if !private {
            let stored = self.store.borrow().permission(&ask.origin, LOCAL_NETWORK).unwrap_or(None);
            if let Some(allow) = stored {
                info!(
                    "tab {}: local network access {} (stored choice)",
                    ask.tab,
                    if allow { "allowed" } else { "blocked" }
                );
                if let Some(v) = self.tab_view(ask.tab) {
                    v.answer_permission(ask.prompt_id, allow);
                }
                return;
            }
        }
        self.permissions.waiting.borrow_mut().push(ask);
        self.show_next_permission_prompt();
    }

    /// Shows the first waiting prompt whose tab is the active one, if no prompt is shown.
    pub(super) fn show_next_permission_prompt(&self) {
        if self.permissions.showing.borrow().is_some() {
            return;
        }
        let active = self.active_id();
        let next = {
            let mut waiting = self.permissions.waiting.borrow_mut();
            let pos = waiting.iter().position(|a| Some(a.tab) == active);
            pos.map(|i| waiting.remove(i))
        };
        let Some(ask) = next else { return };
        let private = self.tab(ask.tab).is_some_and(|t| t.private);
        info!(
            "tab {}: local network access asked by {} ({})",
            ask.tab,
            ask.origin,
            if ask.loopback { "this computer" } else { "local network" }
        );
        let content = PopupContent::Permission {
            tab_id: ask.tab,
            prompt_id: ask.prompt_id,
            origin: ask.origin.clone(),
            loopback: ask.loopback,
            private,
        };
        *self.permissions.showing.borrow_mut() = Some(ask);
        self.show_prompt(content, 0);
    }

    /// The popup's answer.
    pub(super) fn permission_reply(&self, prompt_id: u64, allow: bool, remember: bool) {
        let showing = self.permissions.showing.borrow_mut().take();
        self.hide_popup();
        let Some(ask) = showing.filter(|a| a.prompt_id == prompt_id) else { return };
        let private = self.tab(ask.tab).is_some_and(|t| t.private);
        if remember && !private {
            if let Err(e) = self.store.borrow().permission_set(&ask.origin, LOCAL_NETWORK, allow, now_ms()) {
                crate::log::warning!("cannot store the permission: {e}");
            }
        }
        info!(
            "tab {}: local network access {} for {}{}",
            ask.tab,
            if allow { "allowed" } else { "blocked" },
            ask.origin,
            if remember && !private { " (remembered)" } else { "" }
        );
        if let Some(v) = self.tab_view(ask.tab) {
            v.answer_permission(ask.prompt_id, allow);
        }
        self.show_next_permission_prompt();
    }

    /// The prompt was dismissed without an answer (a click elsewhere, Esc): blocked once, so
    /// the page's request fails instead of waiting forever.
    pub(super) fn permission_dismissed(&self, prompt_id: u64) {
        let showing = self.permissions.showing.borrow_mut().take();
        let Some(ask) = showing.filter(|a| a.prompt_id == prompt_id) else { return };
        info!("tab {}: local network access prompt dismissed (blocked once)", ask.tab);
        if let Some(v) = self.tab_view(ask.tab) {
            v.answer_permission(ask.prompt_id, false);
        }
        self.show_next_permission_prompt();
    }

    /// The engine withdrew the prompt (the page navigated away or the tab closed).
    fn permission_withdrawn(&self, tab: TabId, prompt_id: u64) {
        self.permissions.waiting.borrow_mut().retain(|a| !(a.tab == tab && a.prompt_id == prompt_id));
        let showing_it =
            self.permissions.showing.borrow().as_ref().is_some_and(|a| a.tab == tab && a.prompt_id == prompt_id);
        if showing_it {
            self.permissions.showing.borrow_mut().take();
            self.hide_popup();
            self.show_next_permission_prompt();
        }
    }

    /// The stored decisions of `origin` for krunk://sitedata.
    pub(super) fn site_permissions_json(&self, origin: &str) -> serde_json::Value {
        let list = self.store.borrow().permissions_for(origin).unwrap_or_default();
        let local = list.iter().find(|p| p.kind == LOCAL_NETWORK).map(|p| p.allow);
        serde_json::json!({ "localNetwork": local })
    }
}
