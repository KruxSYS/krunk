//! Shared DevTools domains of a tab (0.12.0).
//!
//! The extensions' content scripts, the userscripts and the AI agent each need the `Page` and
//! `Runtime` domains of a tab (new-document scripts run only while `Page` is on; contexts and
//! binding calls come from `Runtime`). Before 0.12.0 the extension code switched both off when
//! it had no scripts for a tab; now each user holds a bit, and the domains go off only when
//! the last user releases them.

// 0.12.0 spine: parts of this are used once the work packages are wired in (M4).
#![allow(dead_code)]

use super::Browser;
use crate::ffi::cef::View;
use lb_core::tabs::TabId;
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CdpUser {
    Ext = 1,
    Userscripts = 2,
    Agent = 4,
    ScriptsPage = 8,
}

#[derive(Default)]
pub(super) struct CdpHost {
    /// The users of each tab's domains (bits of `CdpUser`).
    users: RefCell<HashMap<TabId, u8>>,
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
            view.devtools_method("Runtime.enable", &json!({}));
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
            view.devtools_method("Runtime.disable", &json!({}));
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
    }
}
