//! Shared DevTools domains of a tab (0.12.0).
//!
//! The extensions' content scripts, the userscripts and the AI agent each need the `Page` and
//! `Runtime` domains of a tab (new-document scripts run only while `Page` is on; contexts and
//! binding calls come from `Runtime`). Before 0.12.0 the extension code switched both off when
//! it had no scripts for a tab; now each user holds a bit, and the domains go off only when
//! the last user releases them.

use super::Browser;
use crate::ffi::cef::View;
use lb_core::tabs::TabId;
use serde_json::json;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CdpUser {
    Ext = 1,
    Userscripts = 2,
    Agent = 4,
    ScriptsPage = 8,
    /// The WASM memory inspector (0.19.0).
    Wasm = 16,
}

#[derive(Default)]
pub(super) struct CdpHost {
    /// The users of each tab's domains (bits of `CdpUser`).
    users: RefCell<HashMap<TabId, u8>>,
    /// 0.27.1: tabs showing a Cloudflare challenge: their Runtime domain stays off. Cloudflare
    /// spots an enabled Runtime domain (it is how automation tools drive a browser) and may
    /// keep challenging; content scripts and userscripts wait for the next document.
    paused: RefCell<HashSet<TabId>>,
}

impl Browser {
    /// `user` needs tab `tab`'s Page and Runtime domains: they go on with the first user.
    pub(super) fn cdp_use(&self, tab: TabId, view: &View, user: CdpUser) {
        let first = {
            let mut users = self.cdp.users.borrow_mut();
            let bits = users.entry(tab).or_insert(0);
            let first = *bits == 0;
            *bits |= user as u8;
            first
        };
        if first {
            view.devtools_method("Page.enable", &json!({}));
            if !self.cdp.paused.borrow().contains(&tab) {
                view.devtools_method("Runtime.enable", &json!({}));
            }
        }
    }

    /// 0.27.1 (APP_CHALLENGE): a tab's document became, or stopped being, a Cloudflare
    /// challenge: its Runtime domain goes off for the challenge and on again after it (when
    /// something uses it).
    pub(super) fn cdp_challenges_changed(&self) {
        let tabs: Vec<(TabId, View, bool)> =
            self.views.borrow().iter().map(|(id, v)| (*id, v.view.clone(), v.net.challenge())).collect();
        for (tab, view, challenge) in tabs {
            let was = self.cdp.paused.borrow().contains(&tab);
            if challenge == was {
                continue;
            }
            let used = self.cdp.users.borrow().contains_key(&tab);
            if challenge {
                self.cdp.paused.borrow_mut().insert(tab);
                if used {
                    crate::log::info!("tab {tab}: Cloudflare challenge, the Runtime domain is off until it passes");
                    view.devtools_method("Runtime.disable", &json!({}));
                }
            } else {
                self.cdp.paused.borrow_mut().remove(&tab);
                if used {
                    view.devtools_method("Runtime.enable", &json!({}));
                }
            }
        }
    }

    /// `user` is done with the domains: they go off with the last user. Returns whether
    /// `user` had them.
    pub(super) fn cdp_release(&self, tab: TabId, view: &View, user: CdpUser) -> bool {
        let (had, last) = {
            let mut users = self.cdp.users.borrow_mut();
            let Some(bits) = users.get_mut(&tab) else { return false };
            let had = *bits & user as u8 != 0;
            *bits &= !(user as u8);
            let last = had && *bits == 0;
            if *bits == 0 {
                users.remove(&tab);
            }
            (had, last)
        };
        if last {
            if !self.cdp.paused.borrow().contains(&tab) {
                view.devtools_method("Runtime.disable", &json!({}));
            }
            self.wasm_contexts_forget(tab);
            view.devtools_method("Page.disable", &json!({}));
        }
        had
    }

    pub(super) fn cdp_has(&self, tab: TabId, user: CdpUser) -> bool {
        self.cdp.users.borrow().get(&tab).is_some_and(|b| b & user as u8 != 0)
    }

    /// The tab's browser is new (or gone): no domain is on.
    pub(super) fn cdp_forget(&self, tab: TabId) {
        self.cdp.users.borrow_mut().remove(&tab);
        self.cdp.paused.borrow_mut().remove(&tab);
        self.wasm_contexts_forget(tab);
    }
}
