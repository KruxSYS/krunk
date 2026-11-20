//! Address-bar suggestions (Krunk 0.5.1): the shell sends what the user typed, the host
//! answers with the rows (`lb_core::omnibox`) and shows them in the popup under the address
//! bar without taking the focus; the shell keeps the keyboard selection and tells the host
//! about it; a click in the popup or Enter in the shell goes to the chosen row.
//!
//! 0.15.0: the search engine's suggestions (`lb_core::search_suggest`) are fetched 150 ms
//! after the last query, without cookies through the normal profile (so the proxy and the
//! VPN apply), never for a private tab or with the setting off, and added under the local
//! rows when they arrive (`Event::AddressSearchSuggestions`). A newer query cancels the
//! request in flight; answers are kept for a while in a small cache.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use lb_core::protocol::{AddressEdit, AddressMenuAction, AddressSuggestion, Event, PopupContent, PopupKind, PopupRect};
use lb_core::{omnibox, search_suggest, urlclean};

use super::Browser;
use crate::ffi::cef::fetch::{self, FetchError, Fetched, JobId, Req};
use crate::ffi::cef::post_ui_delayed;
use crate::log::info;

/// Wait after the last query before the engine is asked (0.15.0).
const SUGGEST_DEBOUNCE_MS: i64 = 150;
/// A suggest request quiet this long is given up.
const SUGGEST_TIMEOUT_MS: i64 = 3000;
/// Answers kept (suggest URL → suggestions), most recent last.
const SUGGEST_CACHE: usize = 32;

#[derive(Default)]
pub(super) struct OmniboxHost {
    /// The rows shown now (for a selection change without a new query).
    entries: RefCell<Vec<AddressSuggestion>>,
    /// The selected text when the context menu opened (Cut/Copy), and whether the bar
    /// had been edited (Undo).
    menu_selection: RefCell<Option<String>>,
    /// 0.15.0: the text the rows are for, the popup's place and the selected row (to show
    /// the rows again when the engine's suggestions arrive).
    text: RefCell<String>,
    rect: Cell<Option<PopupRect>>,
    selected: Cell<Option<usize>>,
    /// Bumped by every query and close: an older timer or answer sees it changed and stops.
    suggest_gen: Cell<u64>,
    /// The suggest request in flight.
    suggest_job: Cell<Option<JobId>>,
    suggest_cache: RefCell<VecDeque<(String, Vec<String>)>>,
    /// 0.26.0: the origin last preconnected for the top row, and when.
    preconnected: RefCell<Option<(String, std::time::Instant)>>,
}

/// The same origin is not asked again within this time (the engine keeps an idle socket
/// open for about this long).
const PRECONNECT_AGAIN_MS: u128 = 10_000;

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
        let started = std::time::Instant::now();
        let (entries, inline) = {
            let template = self.settings.borrow().search_template().to_string();
            omnibox::query(&self.store.borrow(), &text, &template)
        };
        let ms = started.elapsed().as_millis();
        if ms > 10 {
            info!("address bar: suggestions took {ms} ms to find");
        }
        let selected = inline.as_ref().map(|i| i.index);
        // 0.26.0: the row Enter would open (the inline completion, else the first row) gets an
        // early connection, as in Chrome.
        if let Some(url) = inline.as_ref().map(|i| i.url.clone()).or_else(|| entries.first().map(|e| e.url.clone())) {
            self.omnibox_preconnect(&url);
        }
        self.post_shell(&Event::AddressSuggestions { text: text.clone(), entries: entries.clone(), inline });
        if entries.is_empty() {
            self.address_suggest_close();
            return;
        }
        let height = omnibox::height_css(entries.len());
        let popup_rect = PopupRect { x: rect.x, y: rect.y, width: rect.width.max(300.0), height };
        *self.omnibox.entries.borrow_mut() = entries.clone();
        *self.omnibox.text.borrow_mut() = text.clone();
        self.omnibox.rect.set(Some(popup_rect));
        self.omnibox.selected.set(selected);
        self.show_popup(PopupContent::AddressSuggest { entries, selected }, Some(popup_rect));
        self.search_suggest_schedule(&text);
    }

    /// 0.26.0: an early connection to where the top row goes ("Preload pages", never for a
    /// private tab; the engine skips blocked hosts and uses the profile's proxy / VPN).
    fn omnibox_preconnect(&self, url: &str) {
        if !super::preload_effective(&self.settings.borrow()) || self.active_tab().is_none_or(|t| t.private) {
            return;
        }
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return;
        }
        // Measurement builds: KRUNK_PERF_OFF=omnibox-preconnect (0.26.0 A/B).
        #[cfg(feature = "perf-probe")]
        if super::perf_probe::off("omnibox-preconnect") {
            return;
        }
        let Some(origin) = url::Url::parse(url).ok().map(|u| u.origin().ascii_serialization()) else { return };
        {
            let last = self.omnibox.preconnected.borrow();
            if last.as_ref().is_some_and(|(o, t)| *o == origin && t.elapsed().as_millis() < PRECONNECT_AGAIN_MS) {
                return;
            }
        }
        *self.omnibox.preconnected.borrow_mut() = Some((origin, std::time::Instant::now()));
        crate::ffi::cef::preconnect(url);
    }

    /// 0.15.0: asks the search engine about `text` after a pause, if allowed: the setting is
    /// on, the engine has an endpoint, the active tab is not private, the text is words.
    fn search_suggest_schedule(&self, text: &str) {
        let generation = self.search_suggest_stop();
        let url = {
            let s = self.settings.borrow();
            if !s.search_suggestions || !search_suggest::wanted(text, s.search_template()) {
                return;
            }
            search_suggest::suggest_url(s.search_engine, text)
        };
        let Some(url) = url else { return };
        if self.active_tab().is_none_or(|t| t.private) {
            return;
        }
        let cached = self.omnibox.suggest_cache.borrow().iter().find(|(u, _)| *u == url).map(|(_, v)| v.clone());
        if let Some(items) = cached {
            self.search_suggest_show(&items);
            return;
        }
        let weak = self.me.clone();
        post_ui_delayed(SUGGEST_DEBOUNCE_MS, move || {
            if let Some(b) = weak.upgrade() {
                b.search_suggest_fetch(generation, url);
            }
        });
    }

    /// Makes the newer query win: the waiting one and the request in flight are dropped.
    /// Returns the new generation.
    fn search_suggest_stop(&self) -> u64 {
        let generation = self.omnibox.suggest_gen.get() + 1;
        self.omnibox.suggest_gen.set(generation);
        if let Some(job) = self.omnibox.suggest_job.take() {
            fetch::cancel(job);
        }
        generation
    }

    fn search_suggest_fetch(&self, generation: u64, url: String) {
        if generation != self.omnibox.suggest_gen.get() || self.popup_kind() != Some(PopupKind::AddressSuggest) {
            return;
        }
        // The body arrives in pieces (`fetch::request` streams); the status comes with each.
        let body = Rc::new(RefCell::new((0, Vec::new())));
        let sink = body.clone();
        let weak = self.me.clone();
        let req = Req {
            method: "GET",
            url: &url,
            headers: &[],
            body: None,
            idle_timeout_ms: SUGGEST_TIMEOUT_MS,
            max_bytes: search_suggest::MAX_BODY,
        };
        let key = url.clone();
        let job = fetch::request(
            req,
            move |status, chunk| {
                let mut b = sink.borrow_mut();
                b.0 = status;
                b.1.extend_from_slice(chunk);
            },
            move |r: Result<Fetched, FetchError>| {
                let Some(b) = weak.upgrade() else { return };
                let (status, bytes) = std::mem::take(&mut *body.borrow_mut());
                b.search_suggest_done(generation, key, r.map(|f| if status == 0 { f.status } else { status }), bytes);
            },
        );
        if generation == self.omnibox.suggest_gen.get() {
            self.omnibox.suggest_job.set(Some(job));
        }
    }

    fn search_suggest_done(&self, generation: u64, url: String, r: Result<i32, FetchError>, body: Vec<u8>) {
        let current = generation == self.omnibox.suggest_gen.get();
        if current {
            self.omnibox.suggest_job.set(None);
        }
        let items = match r {
            Ok(200) => search_suggest::parse(&body),
            Ok(status) => {
                info!("search suggestions: HTTP {status}");
                return;
            }
            Err(FetchError::Cancelled) => return,
            Err(e) => {
                info!("search suggestions: {e:?}");
                return;
            }
        };
        {
            let mut cache = self.omnibox.suggest_cache.borrow_mut();
            cache.retain(|(u, _)| *u != url);
            if cache.len() >= SUGGEST_CACHE {
                cache.pop_front();
            }
            cache.push_back((url, items.clone()));
        }
        if current {
            self.search_suggest_show(&items);
        }
    }

    /// Adds the suggestions under the rows shown (if the list is still open).
    fn search_suggest_show(&self, items: &[String]) {
        if self.popup_kind() != Some(PopupKind::AddressSuggest) {
            return;
        }
        let Some(rect) = self.omnibox.rect.get() else { return };
        let text = self.omnibox.text.borrow().clone();
        let entries = {
            let template = self.settings.borrow().search_template().to_string();
            let local = self.omnibox.entries.borrow();
            match omnibox::with_search_suggestions(&local, &text, items, &template) {
                Some(e) => e,
                None => return,
            }
        };
        let selected = self.omnibox.selected.get().filter(|i| *i < entries.len());
        let popup_rect = PopupRect { height: omnibox::height_css(entries.len()), ..rect };
        *self.omnibox.entries.borrow_mut() = entries.clone();
        self.post_shell(&Event::AddressSearchSuggestions { text, entries: entries.clone() });
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
                    self.post_shell(&Event::AddressNavigated);
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
        self.omnibox.selected.set(selected);
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
        self.post_shell(&Event::AddressNavigated);
        self.post_shell(&Event::BlurAddressBar);
        self.focus_page_after_navigation();
    }

    /// Hides the suggestions if they are what the popup shows.
    pub(super) fn address_suggest_close(&self) {
        self.search_suggest_stop();
        self.omnibox.entries.borrow_mut().clear();
        if self.popup_kind() == Some(PopupKind::AddressSuggest) {
            self.hide_popup();
        }
    }
}
