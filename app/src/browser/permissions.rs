//! Site permissions (Krunk 0.5.0: local network access; 0.6.0: camera, microphone,
//! location, notifications, clipboard reading, MIDI and screen sharing). The engine asks
//! the embedder through a permission prompt or a media access request; Krunk answers from
//! the user's stored choice for the site, from the per-permission default (Ask or Block),
//! or shows its own prompt ("Allow" / "Block", remembered for the site unless the tab is
//! private). A prompt for a tab that is not active waits until the tab is; dismissing the
//! prompt blocks once. Notifications never reach the embedder, so Krunk's shim in the page
//! asks instead and the answer is written into the engine's content settings.

use std::cell::RefCell;

use lb_core::permissions::{kinds_from_media_bits, kinds_from_prompt_bits, Kind, PermissionState, SitePermission};
use lb_core::protocol::PopupContent;
use lb_core::settings::PermissionDefault;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;

use super::Browser;
use crate::ffi::cef::View;
use crate::log::{info, warning};

/// How the engine asked (and how the answer goes back).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// `on_show_permission_prompt` with this prompt id.
    Prompt(u64),
    /// `on_request_media_access_permission` with this request id.
    Media(u64),
    /// The page's `Notification.requestPermission()` (answered through content settings).
    Notification,
}

/// A request the engine (or the page) opened for a tab.
#[derive(Debug, Clone)]
struct Ask {
    tab: TabId,
    source: Source,
    /// The id the popup replies with (unique over all sources).
    id: u64,
    origin: String,
    kinds: Vec<Kind>,
    loopback: bool,
}

#[derive(Default)]
pub(super) struct PermissionHost {
    /// Prompts waiting for their tab to become active (or for the current prompt to close).
    waiting: RefCell<Vec<Ask>>,
    /// The prompt shown now.
    showing: RefCell<Option<Ask>>,
    next_id: std::cell::Cell<u64>,
}

impl Browser {
    pub(super) fn hook_permissions(&self, id: TabId, c: &View) {
        let weak = self.me.clone();
        c.on_permission(move |prompt_id, origin, bits| {
            let Some(b) = weak.upgrade() else { return };
            match kinds_from_prompt_bits(bits) {
                Ok(kinds) => {
                    let loopback = lb_core::permissions::network_request(bits).is_some_and(|r| r.loopback);
                    b.permission_requested(id, Source::Prompt(prompt_id), origin, kinds, loopback);
                }
                Err(unknown) => {
                    // Not left hanging (0.5.2 did that): denied, and logged for the next release.
                    warning!("tab {id}: permission prompt with unknown bits {unknown:#x} from {origin}: denied");
                    if let Some(v) = b.tab_view(id) {
                        v.answer_permission(prompt_id, false);
                    }
                }
            }
        });
        let weak = self.me.clone();
        c.on_media_access(move |req_id, origin, bits| {
            if let Some(b) = weak.upgrade() {
                let kinds = kinds_from_media_bits(bits);
                if kinds.is_empty() {
                    if let Some(v) = b.tab_view(id) {
                        v.answer_media(req_id, false);
                    }
                    return;
                }
                b.permission_requested(id, Source::Media(req_id), origin, kinds, false);
            }
        });
        let weak = self.me.clone();
        c.on_permission_gone(move |prompt_id| {
            if let Some(b) = weak.upgrade() {
                b.permission_withdrawn(id, Source::Prompt(prompt_id));
            }
        });
        let weak = self.me.clone();
        c.on_media_use(move |video, audio| {
            if let Some(b) = weak.upgrade() {
                b.media_use_changed(id, video, audio);
            }
        });
    }

    /// The page called `Notification.requestPermission()` (through Krunk's shim).
    pub(super) fn notification_permission_requested(&self, id: TabId, origin: &str) {
        self.permission_requested(id, Source::Notification, origin.to_string(), vec![Kind::Notifications], false);
    }

    fn permission_requested(&self, tab: TabId, source: Source, origin: String, kinds: Vec<Kind>, loopback: bool) {
        let private = self.tab(tab).is_some_and(|t| t.private);
        // Stored choices (never in private tabs), then the defaults.
        let stored: Vec<Option<bool>> = kinds
            .iter()
            .map(|k| if private { None } else { self.store.borrow().permission(&origin, k.key()).unwrap_or(None) })
            .collect();
        if stored.iter().all(|s| s.is_some()) {
            let allow = stored.iter().all(|s| *s == Some(true));
            info!(
                "tab {tab}: {} {} for {origin} (stored choice)",
                describe(&kinds),
                if allow { "allowed" } else { "blocked" }
            );
            return self.answer(tab, source, &origin, allow);
        }
        let blocked_by_default = {
            let s = self.settings.borrow();
            kinds.iter().zip(&stored).any(|(k, st)| st.is_none() && s.permissions.get(*k) == PermissionDefault::Block)
        };
        if blocked_by_default {
            info!("tab {tab}: {} blocked for {origin} (default)", describe(&kinds));
            return self.answer(tab, source, &origin, false);
        }
        let id = self.permissions.next_id.get() + 1;
        self.permissions.next_id.set(id);
        self.permissions.waiting.borrow_mut().push(Ask { tab, source, id, origin, kinds, loopback });
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
        info!("tab {}: {} asked by {}", ask.tab, describe(&ask.kinds), ask.origin);
        let content = PopupContent::Permission {
            kinds: ask.kinds.clone(),
            tab_id: ask.tab,
            prompt_id: ask.id,
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
        let Some(ask) = showing.filter(|a| a.id == prompt_id) else { return };
        let private = self.tab(ask.tab).is_some_and(|t| t.private);
        if remember && !private {
            for k in &ask.kinds {
                if let Err(e) = self.store.borrow().permission_set(&ask.origin, k.key(), allow, now_ms()) {
                    warning!("cannot store the permission: {e}");
                }
            }
        }
        info!(
            "tab {}: {} {} for {}{}",
            ask.tab,
            describe(&ask.kinds),
            if allow { "allowed" } else { "blocked" },
            ask.origin,
            if remember && !private { " (remembered)" } else { "" }
        );
        self.answer(ask.tab, ask.source, &ask.origin, allow);
        self.show_next_permission_prompt();
    }

    /// The prompt was dismissed without an answer (a click elsewhere, Esc): blocked once, so
    /// the page's request fails instead of waiting forever (notifications stay "default").
    pub(super) fn permission_dismissed(&self, prompt_id: u64) {
        let showing = self.permissions.showing.borrow_mut().take();
        let Some(ask) = showing.filter(|a| a.id == prompt_id) else { return };
        info!("tab {}: {} prompt dismissed (blocked once)", ask.tab, describe(&ask.kinds));
        if ask.source != Source::Notification {
            self.answer(ask.tab, ask.source, &ask.origin, false);
        }
        self.show_next_permission_prompt();
    }

    /// Sends the answer to the engine (or, for notifications, writes the content setting).
    fn answer(&self, tab: TabId, source: Source, origin: &str, allow: bool) {
        let Some(v) = self.tab_view(tab) else { return };
        match source {
            Source::Prompt(id) => {
                v.answer_permission(id, allow);
            }
            Source::Media(id) => {
                v.answer_media(id, allow);
            }
            Source::Notification => v.set_notifications_setting(origin, Some(allow)),
        }
    }

    /// The engine withdrew the prompt (the page navigated away or the tab closed).
    fn permission_withdrawn(&self, tab: TabId, source: Source) {
        self.permissions.waiting.borrow_mut().retain(|a| !(a.tab == tab && a.source == source));
        let showing_it = self.permissions.showing.borrow().as_ref().is_some_and(|a| a.tab == tab && a.source == source);
        if showing_it {
            self.permissions.showing.borrow_mut().take();
            self.hide_popup();
            self.show_next_permission_prompt();
        }
    }

    /// A tab navigated: the engine's own notification setting for the new origin follows
    /// Krunk's stored choice or default (the engine reads it for `Notification.permission`).
    pub(super) fn apply_notification_setting(&self, tab: TabId, url: &str) {
        let Some(origin) = lb_core::urlclean::origin_of(url) else { return };
        if self.tab(tab).is_some_and(|t| t.private) {
            return;
        }
        let stored = self.store.borrow().permission(&origin, Kind::Notifications.key()).unwrap_or(None);
        let value = match stored {
            Some(allow) => Some(allow),
            None if self.settings.borrow().permissions.notifications == PermissionDefault::Block => Some(false),
            None => None,
        };
        if let Some(v) = self.tab_view(tab) {
            v.set_notifications_setting(&origin, value);
        }
    }

    /// The page started or stopped using the camera / the microphone: the address bar's chip.
    fn media_use_changed(&self, tab: TabId, video: bool, audio: bool) {
        if let Some(v) = self.views.borrow().get(&tab) {
            v.media_use.set((video, audio));
        }
        info!("tab {tab}: camera {} microphone {}", if video { "on" } else { "off" }, if audio { "on" } else { "off" });
        if let Some(win) = self.window_of(tab) {
            self.send_nav_to(win);
        }
    }

    /// The stored decisions of `origin` for krunk://sitedata (`kind → allow`).
    pub(super) fn site_permissions_json(&self, origin: &str) -> serde_json::Value {
        let list = self.store.borrow().permissions_for(origin).unwrap_or_default();
        let mut map = serde_json::Map::new();
        for k in Kind::ALL {
            let v = list.iter().find(|p| p.kind == k.key()).map(|p| serde_json::Value::Bool(p.allow));
            map.insert(k.key().to_string(), v.unwrap_or(serde_json::Value::Null));
        }
        serde_json::Value::Object(map)
    }

    /// The site's permissions for the lock popup: every kind with its state.
    pub(super) fn site_permission_rows(&self, origin: &str) -> Vec<lb_core::certinfo::SitePermissionRow> {
        let list = self.store.borrow().permissions_for(origin).unwrap_or_default();
        Kind::ALL
            .into_iter()
            .map(|k| {
                let state = match list.iter().find(|p| p.kind == k.key()) {
                    Some(p) if p.allow => PermissionState::Allow,
                    Some(_) => PermissionState::Block,
                    None => PermissionState::Ask,
                };
                lb_core::certinfo::SitePermissionRow { kind: k, state }
            })
            .collect()
    }

    /// Every stored per-site decision (Settings → Permissions).
    pub(super) fn permission_exceptions(&self) -> Vec<SitePermission> {
        self.store.borrow().permissions_list().unwrap_or_default()
    }

    /// Sets or removes (`Ask`) a site's permission; notifications also update the engine's
    /// setting for the tabs on that site.
    pub(super) fn permission_set(&self, origin: &str, kind: Kind, state: PermissionState) -> Result<(), String> {
        let Some(origin) = lb_core::urlclean::origin_of(origin) else { return Err("Not a site".into()) };
        let r = match state {
            PermissionState::Ask => self.store.borrow().permission_remove(&origin, kind.key()).map(|_| ()),
            PermissionState::Allow => self.store.borrow().permission_set(&origin, kind.key(), true, now_ms()),
            PermissionState::Block => self.store.borrow().permission_set(&origin, kind.key(), false, now_ms()),
        };
        r.map_err(|e| e.to_string())?;
        info!("permissions: {} for {origin} set to {state:?}", kind.key());
        if kind == Kind::Notifications {
            let tabs: Vec<TabId> = self.views.borrow().keys().copied().collect();
            for id in tabs {
                if let Some(url) = self.tab(id).map(|t| t.url.clone()) {
                    if lb_core::urlclean::origin_of(&url).as_deref() == Some(origin.as_str()) {
                        self.apply_notification_setting(id, &url);
                    }
                }
            }
        }
        Ok(())
    }
}

fn describe(kinds: &[Kind]) -> String {
    kinds.iter().map(|k| k.key()).collect::<Vec<_>>().join("+")
}
