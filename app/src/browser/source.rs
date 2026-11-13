//! A page's source in Krunk's own viewer (0.11.0: "View page source", Ctrl+U and typed
//! `view-source:` opened the engine's temp file in the system text editor), and the page
//! context menu's "Search <engine> for …" for selected text and its open items for a link or a
//! selected address (for a tab's page and, 0.16.0, for the AI panel: [`MenuFrom`]).
//!
//! The viewer (`krunk://source`, `ui/pages/source.js`) names the address in its own URL; the
//! host loads it again the way the tab did (its profile, its cookies), from the cache first.
//! A page that cannot be loaded again (the result of a form POST, a failed request) shows the
//! tab's current document instead, and the viewer says so.

use super::{Browser, TabId};
use crate::ffi::cef::fetch::{self, Fetched};
use crate::ffi::cef::View;
use crate::log::info;
use lb_core::pages;
use lb_core::protocol::Event;
use lb_core::storage::now_ms;
use lb_core::tabs::Position;
use lb_core::urlclean;
use lb_core::windows::WindowId;

/// The viewer reads at most this much.
const MAX_SOURCE: usize = 20 * 1024 * 1024;
const SOURCE_TIMEOUT_MS: i64 = 20_000;

#[derive(serde::Serialize)]
struct Source {
    url: String,
    /// "cache", "network" or "page" (the tab's current document).
    from: &'static str,
    status: Option<i32>,
    mime: String,
    text: String,
}

/// Where a page context menu was opened (0.16.0): its commands open tabs for that place.
#[derive(Debug, Clone, Copy)]
pub(super) enum MenuFrom {
    /// On a tab's page.
    Tab(TabId),
    /// In the AI panel of a window.
    Panel(WindowId),
}

/// For the log: "tab 3", "window 1's agent panel".
impl std::fmt::Display for MenuFrom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MenuFrom::Tab(id) => write!(f, "tab {id}"),
            MenuFrom::Panel(win) => write!(f, "window {win}'s agent panel"),
        }
    }
}

impl Browser {
    /// "View page source" / Ctrl+U on tab `id` (`url`: the page's address, else the tab's):
    /// the viewer in a new tab next to it, private if the tab is.
    pub(super) fn view_source(&self, id: TabId, url: Option<String>) {
        let Some((win, private, tab_url)) =
            self.window_of(id).and_then(|w| self.tab(id).map(|t| (w, t.private, t.url.clone())))
        else {
            return;
        };
        let url = url.unwrap_or(tab_url);
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return;
        }
        let opened = self.ws.borrow_mut().open(
            win,
            &pages::source_url(&url, Some(id)),
            private,
            Position::AfterOpener(id),
            true,
            now_ms(),
        );
        if let Some((_, ops)) = opened {
            self.apply(ops);
        }
    }

    /// Connects the page menu's commands of view `c` (search, the open items, the image items)
    /// to the place the menu is in: a tab, or (0.16.0) a window's AI panel. Both go through
    /// the same functions; `from` says where the new tabs go and whose images they are.
    pub(super) fn hook_page_menu(&self, c: &View, from: MenuFrom) {
        let weak = self.me.clone();
        c.on_search_label(move |sel| weak.upgrade().and_then(|b| b.search_selection_label(sel)));
        let weak = self.me.clone();
        c.on_search_selection(move |sel| {
            if let Some(b) = weak.upgrade() {
                b.menu_used(from);
                b.search_selection(from, &sel);
            }
        });
        let weak = self.me.clone();
        c.on_open_selection(move |how, url| {
            if let Some(b) = weak.upgrade() {
                b.defer(move |b| {
                    b.menu_used(from);
                    b.open_selection(from, how, &url)
                });
            }
        });
        let weak = self.me.clone();
        c.on_image_menu(move |action, url| {
            if let Some(b) = weak.upgrade() {
                b.defer(move |b| {
                    b.menu_used(from);
                    b.image_action(from, action, url)
                });
            }
        });
    }

    /// A command of the menu was chosen. The panel's window becomes the current one (as for
    /// its keys and links): a private tab, a dialog and the clipboard go to the current window.
    fn menu_used(&self, from: MenuFrom) {
        if let MenuFrom::Panel(win) = from {
            self.make_current(win);
        }
    }

    /// The context menu item for `selection`, if any.
    pub(super) fn search_selection_label(&self, selection: &str) -> Option<String> {
        let q = urlclean::selection_query(selection)?;
        Some(urlclean::selection_label(&self.settings.borrow().search_engine_label(), &q))
    }

    /// Where a page menu's new tab goes: the window, whether the tab is private, and its place.
    /// From a tab: next to it, private if it is. From a window's AI panel (never private):
    /// where Settings → New tabs puts new tabs, as for a link clicked in the panel.
    fn menu_place(&self, from: MenuFrom) -> Option<(WindowId, bool, Position)> {
        match from {
            MenuFrom::Tab(id) => {
                let win = self.window_of(id)?;
                let private = self.tab(id)?.private;
                Some((win, private, Position::AfterOpener(id)))
            }
            MenuFrom::Panel(win) => {
                self.frame(win)?;
                Some((win, false, self.new_tab_position(win)))
            }
        }
    }

    /// "Search <engine> for …": the search in a new tab (see [`Browser::menu_place`]).
    pub(super) fn search_selection(&self, from: MenuFrom, selection: &str) {
        let Some(q) = urlclean::selection_query(selection) else { return };
        let Some((win, private, position)) = self.menu_place(from) else { return };
        let url = urlclean::search_url(&q, self.settings.borrow().search_template());
        let opened = self.ws.borrow_mut().open(win, &url, private, position, true, now_ms());
        if let Some((_, ops)) = opened {
            self.apply(ops);
        }
    }

    /// "Open in new tab/window/private tab" for a link or a selected address, and "Copy link
    /// address" (0.15.2). A new tab sits next to the menu's tab and stays private if that is; a
    /// new window is private from a private tab. From the AI panel (0.16.0) see
    /// [`Browser::menu_place`]. Web addresses only: the menu has open items for no others (not
    /// for a `javascript:`, `file:` or `krunk://` link), and none is opened here either.
    pub(super) fn open_selection(&self, from: MenuFrom, how: crate::ffi::cef::OpenIn, url: &str) {
        use crate::ffi::cef::OpenIn;
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return;
        }
        let Some((win, private, position)) = self.menu_place(from) else { return };
        match how {
            OpenIn::Tab => {
                let opened = self.ws.borrow_mut().open(win, url, private, position, true, now_ms());
                if let Some((_, ops)) = opened {
                    self.apply(ops);
                }
            }
            OpenIn::Window => {
                let bounds = self.cascade_bounds();
                let (_, _, ops) = self.ws.borrow_mut().open_window(url, private, bounds, false, now_ms());
                self.apply(ops);
            }
            OpenIn::PrivateTab => self.new_tab(Some(url), true),
            OpenIn::CopyAddress => {
                if let Some(w) = self.window() {
                    crate::ffi::secure::copy_text(w.hwnd(), url);
                }
            }
        }
    }

    /// The viewer asks for its source (`tab`: the viewer's tab, whose URL names the address).
    pub(super) fn source_get(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let Some((url, private)) = tab.and_then(|t| self.tab(t).map(|t| (t.url.clone(), t.private))) else { return };
        let Some((target, from_tab)) = pages::source_target(&url) else {
            self.reply(tab, Event::err(req_id, "Not a page address"));
            return;
        };
        let weak = self.me.clone();
        let opts = fetch::Opts { private, credentials: true, cache_only: true };
        let t2 = target.clone();
        fetch::get_with(&target, MAX_SOURCE, SOURCE_TIMEOUT_MS, opts, move |r| {
            let Some(b) = weak.upgrade() else { return };
            match r {
                Ok(f) if (200..300).contains(&f.status) => b.source_reply(tab, req_id, &t2, "cache", f),
                _ => {
                    let weak = b.me.clone();
                    let t3 = t2.clone();
                    let opts = fetch::Opts { private, credentials: true, cache_only: false };
                    fetch::get_with(&t2, MAX_SOURCE, SOURCE_TIMEOUT_MS, opts, move |r| {
                        let Some(b) = weak.upgrade() else { return };
                        match r {
                            Ok(f) => b.source_reply(tab, req_id, &t3, "network", f),
                            Err(e) => {
                                info!("source: {t3} not loaded again ({e:?}); the tab's document");
                                b.source_from_tab(tab, req_id, &t3, from_tab);
                            }
                        }
                    });
                }
            }
        });
    }

    fn source_reply(&self, tab: Option<TabId>, req_id: Option<u32>, url: &str, from: &'static str, f: Fetched) {
        let text = String::from_utf8_lossy(&f.body).into_owned();
        let src = Source { url: url.to_string(), from, status: Some(f.status), mime: f.mime, text };
        self.reply(tab, Event::ok(req_id, src));
    }

    /// The fallback: the document tab `from_tab` shows, if it still shows `url`.
    fn source_from_tab(&self, tab: Option<TabId>, req_id: Option<u32>, url: &str, from_tab: Option<u32>) {
        let view = from_tab.filter(|t| self.tab(*t).is_some_and(|t| t.url == url)).and_then(|t| self.tab_view(t));
        let Some(view) = view else {
            self.reply(tab, Event::err(req_id, "The page could not be loaded again"));
            return;
        };
        let weak = self.me.clone();
        let url = url.to_string();
        view.main_frame_source(move |html| {
            let Some(b) = weak.upgrade() else { return };
            match html {
                Some(text) => {
                    let src = Source { url, from: "page", status: None, mime: "text/html".into(), text };
                    b.reply(tab, Event::ok(req_id, src));
                }
                None => b.reply(tab, Event::err(req_id, "The page could not be loaded again")),
            }
        });
    }
}
