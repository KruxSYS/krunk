//! Address-bar suggestions (Krunk 0.5.1): the shell sends what the user typed, the host
//! answers with the rows (`lb_core::omnibox`) and shows them in the popup under the address
//! bar without taking the focus; the shell keeps the keyboard selection and tells the host
//! about it; a click in the popup or Enter in the shell goes to the chosen row.

use std::cell::RefCell;

use lb_core::omnibox;
use lb_core::protocol::{AddressSuggestion, Event, PopupContent, PopupKind, PopupRect};

use super::Browser;
use crate::log::info;

#[derive(Default)]
pub(super) struct OmniboxHost {
    /// The rows shown now (for a selection change without a new query).
    entries: RefCell<Vec<AddressSuggestion>>,
}

impl Browser {
    /// What the shell typed; `rect` is the dropdown's place and width (shell CSS px).
    pub(super) fn address_suggest(&self, text: String, rect: PopupRect) {
        let text = text.trim().to_string();
        if text.is_empty() || text.chars().count() > 512 {
            self.address_suggest_close();
            return;
        }
        let entries = {
            let template = self.settings.borrow().search_template().to_string();
            omnibox::suggestions(&self.store.borrow(), &text, &template)
        };
        self.post_shell(&Event::AddressSuggestions { text, entries: entries.clone() });
        if entries.is_empty() {
            self.address_suggest_close();
            return;
        }
        let height = omnibox::height_css(entries.len());
        let popup_rect = PopupRect { x: rect.x, y: rect.y, width: rect.width.max(300.0), height };
        *self.omnibox.entries.borrow_mut() = entries.clone();
        self.show_popup(PopupContent::AddressSuggest { entries, selected: None }, Some(popup_rect));
    }

    /// The shell moved the selection: the popup highlights that row.
    pub(super) fn address_suggest_select(&self, selected: Option<usize>) {
        if self.popup_kind() != Some(PopupKind::AddressSuggest) {
            return;
        }
        let entries = self.omnibox.entries.borrow().clone();
        let selected = selected.filter(|i| *i < entries.len());
        self.show_popup(PopupContent::AddressSuggest { entries, selected }, None);
    }

    /// A row was chosen: go there in the active tab.
    pub(super) fn address_suggest_pick(&self, url: &str) {
        self.address_suggest_close();
        if !lb_core::bookmarks::bookmarkable(url) && !url.starts_with("https://krunk.invalid/") {
            return;
        }
        info!("address bar: suggestion chosen");
        self.navigate_active(url);
        self.post_shell(&Event::BlurAddressBar);
        self.focus_page_after_navigation();
    }

    /// Hides the suggestions if they are what the popup shows.
    pub(super) fn address_suggest_close(&self) {
        self.omnibox.entries.borrow_mut().clear();
        if self.popup_kind() == Some(PopupKind::AddressSuggest) {
            self.hide_popup();
        }
    }
}
