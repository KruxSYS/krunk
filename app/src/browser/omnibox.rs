//! Address-bar suggestions (Krunk 0.5.1): the shell sends what the user typed, the host
//! answers with the rows (`lb_core::omnibox`) and shows them in the popup under the address
//! bar without taking the focus; the shell keeps the keyboard selection and tells the host
//! about it; a click in the popup or Enter in the shell goes to the chosen row.

use std::cell::RefCell;

use lb_core::omnibox;
use lb_core::protocol::{AddressEdit, AddressMenuAction, AddressSuggestion, Event, PopupContent, PopupKind, PopupRect};
use lb_core::urlclean;

use super::Browser;
use crate::log::info;

#[derive(Default)]
pub(super) struct OmniboxHost {
    /// The rows shown now (for a selection change without a new query).
    entries: RefCell<Vec<AddressSuggestion>>,
    /// The selected text when the context menu opened (Cut/Copy), and whether the bar
    /// had been edited (Undo).
    menu_selection: RefCell<Option<String>>,
}

/// The context menu's size in shell CSS px (an upper bound; the popup fits itself).
const ADDRESS_MENU_W: f64 = 230.0;
const ADDRESS_MENU_H: f64 = 260.0;

impl Browser {
    /// What the shell typed; `rect` is the dropdown's place and width (shell CSS px).
    pub(super) fn address_suggest(&self, text: String, rect: PopupRect) {
        let text = text.trim().to_string();
        if text.is_empty() || text.chars().count() > 512 {
            self.address_suggest_close();
            return;
        }
        let (entries, inline) = {
            let template = self.settings.borrow().search_template().to_string();
            omnibox::query(&self.store.borrow(), &text, &template)
        };
        let selected = inline.as_ref().map(|i| i.index);
        self.post_shell(&Event::AddressSuggestions { text, entries: entries.clone(), inline });
        if entries.is_empty() {
            self.address_suggest_close();
            return;
        }
        let height = omnibox::height_css(entries.len());
        let popup_rect = PopupRect { x: rect.x, y: rect.y, width: rect.width.max(300.0), height };
        *self.omnibox.entries.borrow_mut() = entries.clone();
        self.show_popup(PopupContent::AddressSuggest { entries, selected }, Some(popup_rect));
    }

    /// Right-click in the address bar: the edit menu, with the items the state allows.
    pub(super) fn show_address_menu(&self, x: f64, y: f64, selection: String, has_text: bool, dirty: bool) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let Some(main) = self.window() else { return };
        let clip =
            crate::ffi::secure::clipboard_text(main.hwnd()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        let paste_is_url = clip.as_deref().is_some_and(|t| {
            let template = self.settings.borrow().search_template().to_string();
            urlclean::input_to_url(t, &template) != urlclean::search_url(t, &template)
        });
        let has_selection = !selection.is_empty();
        *self.omnibox.menu_selection.borrow_mut() = Some(selection);
        self.address_suggest_close();
        let content = PopupContent::AddressMenu {
            can_undo: dirty,
            can_cut: has_selection,
            can_copy: has_selection,
            can_paste: clip.is_some(),
            paste_is_url,
            can_delete: has_selection,
            can_select_all: has_text,
        };
        let rect = PopupRect {
            x: x.clamp(0.0, 8192.0),
            y: y.clamp(0.0, 8192.0),
            width: ADDRESS_MENU_W,
            height: ADDRESS_MENU_H,
        };
        self.show_popup(content, Some(rect));
    }

    /// A choice in the address bar's menu.
    pub(super) fn address_menu_action(&self, action: AddressMenuAction) {
        if self.popup_kind() != Some(PopupKind::AddressMenu) {
            return;
        }
        let selection = self.omnibox.menu_selection.borrow_mut().take().unwrap_or_default();
        self.hide_popup();
        let Some(main) = self.window() else { return };
        let edit = match action {
            AddressMenuAction::Undo => Some(AddressEdit::Undo),
            AddressMenuAction::Copy => {
                crate::ffi::secure::copy_text(main.hwnd(), &selection);
                None
            }
            AddressMenuAction::Cut => {
                crate::ffi::secure::copy_text(main.hwnd(), &selection);
                Some(AddressEdit::DeleteSelection)
            }
            AddressMenuAction::Delete => Some(AddressEdit::DeleteSelection),
            AddressMenuAction::SelectAll => Some(AddressEdit::SelectAll),
            AddressMenuAction::Paste => {
                crate::ffi::secure::clipboard_text(main.hwnd()).map(|text| AddressEdit::Insert { text })
            }
            AddressMenuAction::PasteAndGo => {
                if let Some(text) = crate::ffi::secure::clipboard_text(main.hwnd()).filter(|t| !t.trim().is_empty()) {
                    info!("address bar: paste and go");
                    self.navigate_input(&text);
                    self.post_shell(&Event::BlurAddressBar);
                    self.focus_page_after_navigation();
                }
                None
            }
        };
        if let Some(edit) = edit {
            self.post_shell(&Event::AddressEdit { edit });
        }
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
