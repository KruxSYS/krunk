//! The back and forward buttons' history menus (0.8.0, B; the rules are in `lb_core::navmenu`).
//! Right-click on an enabled button asks the engine for the tab's navigation list, shows the
//! entries before (Back, newest first) or after (Forward) the current one, and a click jumps to
//! exactly that entry. Nothing is written anywhere: the list comes from the engine each time
//! and lives only in the open menu (also in private windows). No address is logged.

use super::Browser;
use lb_core::navmenu::{self, Item};
use lb_core::pages;
use lb_core::protocol::{PopupContent, PopupRect};
use lb_core::tabs::TabId;
use lb_core::urlclean;

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
}
