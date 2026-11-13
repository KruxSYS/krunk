//! The back and forward buttons' history menus (0.8.0, B; the rules are in `lb_core::navmenu`).
//! Right-click on an enabled button asks the engine for the tab's navigation list, shows the
//! entries before (Back, newest first) or after (Forward) the current one, and a click jumps to
//! exactly that entry. Nothing is written anywhere: the list comes from the engine each time
//! and lives only in the open menu (also in private windows). No address is logged.

use super::Browser;
use lb_core::navmenu::{self, Item};
use lb_core::pages;
use lb_core::protocol::{PopupContent, PopupRect};
use lb_core::tabs::{Position, TabId};
use lb_core::urlclean;
use std::cell::Cell;

thread_local! {
    /// The engine had no clone command (an engine without the 0.18.0 patch): said once.
    static CLONE_MISSING_LOGGED: Cell<bool> = const { Cell::new(false) };
}

/// How often (every 150 ms) the new tab is tried while its browser is being created.
const CLONE_TRIES: u32 = 40;

impl Browser {
    pub(super) fn show_nav_menu(&self, forward: bool, rect: PopupRect) {
        let Some(id) = self.active_id() else { return };
        let enabled = self
            .views
            .try_borrow()
            .ok()
            .and_then(|v| v.get(&id).map(|t| if forward { t.can_go_forward } else { t.can_go_back }))
            .unwrap_or(false);
        // A disabled button shows no menu (the shell doesn't ask either).
        if !enabled || !rect.is_valid() {
            return;
        }
        let Some(view) = self.tab_view(id) else { return };
        let weak = self.me.clone();
        view.devtools_call("Page.getNavigationHistory", &serde_json::json!({}), move |r| {
            let Some(b) = weak.upgrade() else { return };
            let Some((current, entries)) = r.ok().as_ref().and_then(navmenu::parse_history) else {
                crate::log::warning!("history menu: the tab's navigation list could not be read");
                return;
            };
            // The user may have switched tabs meanwhile: the menu belongs to the active tab only.
            if b.active_id() != Some(id) {
                return;
            }
            let items = b.nav_items(navmenu::entries_for(current, &entries, forward));
            if items.is_empty() {
                return;
            }
            *b.nav_offer.borrow_mut() = Some((id, navmenu::offered_ids(&items)));
            let rect = lb_core::layout::nav_menu_rect(rect.x, rect.y, items.len());
            b.show_popup(PopupContent::NavMenu { tab_id: id, forward, items }, Some(rect));
        });
    }

    fn nav_items(&self, entries: Vec<&navmenu::Entry>) -> Vec<Item> {
        let store = self.store.try_borrow().ok();
        entries
            .into_iter()
            .map(|e| {
                let shown = pages::display_url(&e.url);
                let icon = urlclean::host_of(&e.url)
                    .filter(|_| e.url.starts_with("http://") || e.url.starts_with("https://"))
                    .and_then(|h| store.as_ref().and_then(|s| s.favicon_url(&h).ok().flatten()));
                Item { entry_id: e.id, label: navmenu::label(&e.title, &shown), url: shown, icon }
            })
            .collect()
    }

    /// A choice in a history menu: only an entry the menu offered for that tab.
    pub(super) fn nav_to_entry(&self, tab_id: TabId, entry_id: i64) {
        self.hide_popup();
        let offered = self.nav_offer.borrow_mut().take();
        if !offered.is_some_and(|(t, ids)| t == tab_id && ids.contains(&entry_id)) {
            return;
        }
        if let Some(view) = self.tab_view(tab_id) {
            view.devtools_call("Page.navigateToHistoryEntry", &serde_json::json!({ "entryId": entry_id }), |r| {
                if r.is_err() {
                    crate::log::warning!("history menu: the jump failed");
                }
            });
            view.focus();
        }
    }

    /// Middle-click in a history menu (0.18.0): entry `entry_id` of tab `tab_id` opens in a new
    /// background tab next to it, with a copy of the tab's whole back/forward list, so Back and
    /// Forward work from there. The source tab stays where it is, the menu stays open. Only an
    /// entry the open menu offered for that tab.
    pub(super) fn nav_to_entry_new_tab(&self, tab_id: TabId, entry_id: i64) {
        let offered = self.nav_offer.borrow().as_ref().is_some_and(|(t, ids)| *t == tab_id && ids.contains(&entry_id));
        if !offered {
            return;
        }
        let Some(view) = self.tab_view(tab_id) else { return };
        let weak = self.me.clone();
        let source = view.clone();
        // The entry's address, for the fallback; the source's DevTools target id for the clone.
        view.devtools_call("Page.getNavigationHistory", &serde_json::json!({}), move |r| {
            let Some(url) = r
                .ok()
                .as_ref()
                .and_then(navmenu::parse_history)
                .and_then(|(_, entries)| navmenu::entry_url(&entries, entry_id).map(str::to_string))
            else {
                crate::log::warning!("history menu: the entry to open in a new tab was not found");
                return;
            };
            source.devtools_call("Target.getTargetInfo", &serde_json::json!({}), move |r| {
                let target = r.ok().and_then(|v| v["targetInfo"]["targetId"].as_str().map(str::to_string));
                if let Some(b) = weak.upgrade() {
                    b.open_history_clone(tab_id, entry_id, &url, target);
                }
            });
        });
    }

    fn open_history_clone(&self, source: TabId, entry_id: i64, url: &str, target: Option<String>) {
        let Some(win) = self.window_of(source) else { return };
        let Some(private) = self.ws.borrow().tab(source).map(|t| t.private) else { return };
        // The new tab starts blank (a fresh history the engine can replace); without a target
        // id it can only load the address.
        let first = if target.is_some() { "about:blank" } else { url };
        self.popup_hold_until.set(lb_core::storage::now_ms() + 600);
        let opened = self.ws.borrow_mut().open(
            win,
            first,
            private,
            Position::AfterOpener(source),
            false,
            lb_core::storage::now_ms(),
        );
        let Some((id, ops)) = opened else { return };
        self.apply(ops);
        self.refocus_dropdown();
        if let Some(target) = target {
            self.clone_history_into(id, target, entry_id, url.to_string(), 0);
        }
    }

    /// Asks the engine to give tab `id` the history of target `target` and go to `entry_id`.
    /// Retried while the new tab's browser is still being created; an engine without the
    /// command (or any other refusal) leaves the new tab at the entry's address only.
    fn clone_history_into(&self, id: TabId, target: String, entry_id: i64, url: String, tries: u32) {
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(150, move || {
            let Some(b) = weak.upgrade() else { return };
            let Some(view) = b.tab_view(id) else {
                if tries < CLONE_TRIES {
                    b.clone_history_into(id, target, entry_id, url, tries + 1);
                }
                return;
            };
            let params = serde_json::json!({ "targetId": target, "entryId": entry_id });
            let weak = b.me.clone();
            let again = view.clone();
            view.devtools_call(navmenu::CLONE_COMMAND, &params, move |r| {
                let Err(e) = r else {
                    crate::log::info!("history menu: tab {id} opened with the history of its tab");
                    return;
                };
                let Some(b) = weak.upgrade() else { return };
                if e == "the page is not open" && tries < CLONE_TRIES {
                    b.clone_history_into(id, target, entry_id, url, tries + 1);
                    return;
                }
                if navmenu::clone_unsupported(&e) {
                    if !CLONE_MISSING_LOGGED.with(|l| l.replace(true)) {
                        crate::log::warning!(
                            "history menu: this engine cannot copy a tab's history; the new tab opens the page only"
                        );
                    }
                } else {
                    crate::log::warning!("history menu: the history could not be copied ({e}); the page only");
                }
                again.navigate(&url);
            });
        });
    }
}
