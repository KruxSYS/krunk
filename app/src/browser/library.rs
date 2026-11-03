//! History, bookmarks, new-tab tiles and clearing browsing data.

use super::Browser;
use crate::ffi::dialogs;
use crate::ffi::webview::ClearKinds;
use crate::log::{info, warning};
use lb_core::bookmarks::bookmarkable;
use lb_core::history;
use lb_core::layout;
use lb_core::netscape;
use lb_core::protocol::{ClearData, Event, PopupContent, PopupRect, Request, Sender};
use lb_core::storage::{now_ms, DbError, BOOKMARKS_BAR};
use lb_core::tabs::TabId;
use lb_core::urlclean;
use serde::Serialize;

/// Number of tiles on the new-tab page.
const TOP_SITES: usize = 12;

impl Browser {
    /// Requests of the built-in pages and the shell's library features.
    pub(super) fn dispatch_library(&self, sender: Sender, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        let now = now_ms();
        match request {
            Request::ToggleBookmark => self.toggle_bookmark(),
            Request::ShowBookmarkFolder { id, rect } => self.show_bookmark_folder(id, rect),
            Request::Find { query, backwards } => self.find(&query, backwards),
            Request::FindClose => {
                if let Some(id) = self.model.borrow().active_id() {
                    self.stop_find(id);
                }
            }

            // ---- new tab page ----
            Request::GetTopSites => self.reply_db(tab, req_id, self.store.borrow().top_sites(TOP_SITES, now)),
            Request::PinSite { url, title } => self.reply_db(tab, req_id, self.store.borrow().pin_site(&url, &title)),
            Request::UnpinSite { url } => self.reply_db(tab, req_id, self.store.borrow().unpin_site(&url)),
            Request::HideSite { url } => self.reply_db(tab, req_id, self.store.borrow().hide_site(&url)),

            // ---- history ----
            Request::QueryHistory { text, before, limit } => {
                self.reply_db(tab, req_id, self.store.borrow().query_history(&text, before, limit))
            }
            Request::DeleteHistory { ids } => self.reply_db(tab, req_id, self.store.borrow_mut().delete_visits(&ids)),
            Request::DeleteHistoryRange { from, to } => {
                self.reply_db(tab, req_id, self.store.borrow_mut().delete_history_range(from, to))
            }
            Request::ClearBrowsingData { data } => self.clear_browsing_data(tab, req_id, data),

            // ---- bookmarks ----
            Request::GetBookmarks => self.reply_db(tab, req_id, self.store.borrow().bookmark_tree()),
            Request::SearchBookmarks { text } => {
                self.reply_db(tab, req_id, self.store.borrow().search_bookmarks(&text))
            }
            Request::AddBookmark { parent, title, url } => {
                let r = self.store.borrow().add_bookmark(parent, &title, &url);
                self.bookmarks_changed(tab, req_id, r);
            }
            Request::AddFolder { parent, title } => {
                let r = self.store.borrow().add_folder(parent, &title);
                self.bookmarks_changed(tab, req_id, r);
            }
            Request::EditBookmark { id, title, url } => {
                let r = self.store.borrow().edit_bookmark(id, &title, url.as_deref());
                self.bookmarks_changed(tab, req_id, r);
            }
            Request::MoveBookmark { id, parent, index } => {
                let r = self.store.borrow_mut().move_bookmark(id, parent, index);
                self.bookmarks_changed(tab, req_id, r);
            }
            Request::DeleteBookmark { id } => {
                let r = self.store.borrow_mut().delete_bookmark(id);
                self.bookmarks_changed(tab, req_id, r);
            }
            Request::ImportBookmarks => self.import_bookmarks(tab, req_id),
            Request::ExportBookmarks => self.export_bookmarks(tab, req_id),

            // ---- downloads ----
            r @ (Request::GetDownloads | Request::ClearDownloads | Request::DownloadAction { .. }) => {
                self.dispatch_downloads(tab, req_id, r)
            }

            other => self.not_available(sender, tab, req_id, &other),
        }
    }

    /// Replies with a database result (errors become an error reply).
    pub(super) fn reply_db<T: Serialize>(&self, tab: Option<TabId>, req_id: Option<u32>, r: Result<T, DbError>) {
        match r {
            Ok(v) => self.reply(tab, Event::ok(req_id, v)),
            Err(e) => {
                warning!("database: {e}");
                self.reply(tab, Event::err(req_id, friendly_db_error(&e)));
            }
        }
    }

    fn bookmarks_changed<T: Serialize>(&self, tab: Option<TabId>, req_id: Option<u32>, r: Result<T, DbError>) {
        self.reply_db(tab, req_id, r);
        self.send_bookmarks_bar();
        self.send_nav();
    }

    // ------------------------------------------------------------ history

    /// A top-level navigation finished: record it (unless private/internal).
    pub(super) fn record_visit(&self, id: TabId) {
        let (url, title, private) = {
            let m = self.model.borrow();
            let Some(t) = m.get(id) else { return };
            (t.url.clone(), t.title.clone(), t.private)
        };
        if let Some(clean) = history::visit_url(&url, private) {
            if let Ok(store) = self.store.try_borrow() {
                if let Err(e) = store.record_visit(&clean, &title, now_ms()) {
                    warning!("history: {e}");
                }
            }
        }
    }

    /// The page title changed: update the stored history title.
    pub(super) fn record_title(&self, id: TabId) {
        let (url, title, private) = {
            let m = self.model.borrow();
            let Some(t) = m.get(id) else { return };
            (t.url.clone(), t.title.clone(), t.private)
        };
        if let Some(clean) = history::visit_url(&url, private) {
            if let Ok(store) = self.store.try_borrow() {
                let _ = store.set_title(&clean, &title);
            }
        }
    }

    /// Keeps the favicon for the new-tab page and bookmarks (not for private tabs).
    pub(super) fn cache_favicon(&self, id: TabId, png: &[u8]) {
        let (url, private) = {
            let m = self.model.borrow();
            let Some(t) = m.get(id) else { return };
            (t.url.clone(), t.private)
        };
        if private || png.is_empty() || history::visit_url(&url, false).is_none() {
            return;
        }
        if let (Some(host), Ok(store)) = (urlclean::host_of(&url), self.store.try_borrow()) {
            let _ = store.set_favicon(&host, png, now_ms());
        }
    }

    fn clear_browsing_data(&self, tab: Option<TabId>, req_id: Option<u32>, data: ClearData) {
        let now = now_ms();
        if data.history {
            let r = match data.since {
                Some(since) => self.store.borrow_mut().delete_history_range(since, now + 1).map(|_| ()),
                None => self.store.borrow_mut().clear_history(),
            };
            if let Err(e) = r {
                warning!("clear history: {e}");
            }
            if data.since.is_none() {
                let _ = self.store.borrow().vacuum();
            }
        }
        if data.downloads {
            self.downloads.borrow_mut().clear_finished();
            self.save_downloads();
            self.push_downloads();
        }
        let kinds = ClearKinds { cookies: data.cookies, cache: data.cache, site_data: data.site_data };
        info!(
            "clearing browsing data (history {}, cookies {}, cache {}, site data {})",
            data.history, data.cookies, data.cache, data.site_data
        );
        // The shell uses the default profile, so it can clear it.
        let Some(shell) = self.shell() else {
            self.reply(tab, Event::err(req_id, "Browser not ready"));
            return;
        };
        let weak = self.me.clone();
        shell.clear_browsing_data(kinds, data.since, move |ok| {
            if let Some(b) = weak.upgrade() {
                b.reply(tab, if ok { Event::ok(req_id, ()) } else { Event::err(req_id, "Clearing site data failed") });
            }
        });
    }

    // ------------------------------------------------------------ bookmarks

    /// Star button / Ctrl+D: bookmark the active page in the bar, or remove it.
    pub(super) fn toggle_bookmark(&self) {
        let (url, title) = match self.model.borrow().active() {
            Some(t) => (t.url.clone(), t.title.clone()),
            None => return,
        };
        if !bookmarkable(&url) {
            return;
        }
        let r = self.store.borrow_mut().toggle_bookmark(&url, &title);
        if let Err(e) = r {
            warning!("bookmark: {e}");
        }
        self.send_bookmarks_bar();
        self.send_nav();
    }

    pub(super) fn send_bookmarks_bar(&self) {
        let items = self.store.borrow().bar_items(BOOKMARKS_BAR).unwrap_or_default();
        self.post_shell(&Event::BookmarksBar { items });
    }

    /// Opens a bar folder in the popup: under its button (`rect` from the
    /// shell) or in place of the current folder (`None`, from the popup).
    fn show_bookmark_folder(&self, id: i64, rect: Option<PopupRect>) {
        let Ok(Some(folder)) = self.store.borrow().bookmark(id) else { return };
        if !folder.is_folder {
            return;
        }
        let items = self.store.borrow().bar_items(id).unwrap_or_default();
        let anchor = match rect {
            Some(r) if r.is_valid() => {
                self.folder_anchor.set(Some(r));
                r
            }
            Some(_) => return,
            None => match self.folder_anchor.get() {
                Some(a) => a,
                None => return,
            },
        };
        let popup_rect = layout::folder_popup_rect(anchor, items.len());
        self.show_popup(PopupContent::BookmarkFolder { id, title: folder.title, items }, Some(popup_rect));
    }

    fn import_bookmarks(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let Some(w) = self.window() else { return };
        let Some(path) = dialogs::open_file(w.hwnd(), "Import bookmarks", ("Bookmark files (HTML)", "*.html;*.htm"))
        else {
            self.reply(tab, Event::ok(req_id, ()));
            return;
        };
        let result = std::fs::metadata(&path)
            .map_err(|e| e.to_string())
            .and_then(|m| {
                if m.len() as usize > netscape::MAX_IMPORT_BYTES {
                    Err("The file is too large".to_string())
                } else {
                    std::fs::read(&path).map_err(|e| e.to_string())
                }
            })
            .and_then(|bytes| self.store.borrow().import_bookmarks(&String::from_utf8_lossy(&bytes)));
        match result {
            Ok(stats) => {
                info!("imported {} bookmarks, {} folders", stats.bookmarks, stats.folders);
                self.reply(
                    tab,
                    Event::ok(
                        req_id,
                        serde_json::json!({"bookmarks": stats.bookmarks, "folders": stats.folders, "skipped": stats.skipped}),
                    ),
                );
                self.send_bookmarks_bar();
                self.send_nav();
            }
            Err(e) => self.reply(tab, Event::err(req_id, format!("Import failed: {e}"))),
        }
    }

    fn export_bookmarks(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let Some(w) = self.window() else { return };
        let Some(path) = dialogs::save_file(
            w.hwnd(),
            "Export bookmarks",
            "bookmarks.html",
            ("Bookmark files (HTML)", "*.html"),
            None,
        ) else {
            self.reply(tab, Event::ok(req_id, ()));
            return;
        };
        let result = netscape::export(&self.store.borrow())
            .map_err(|e| e.to_string())
            .and_then(|html| std::fs::write(&path, html).map_err(|e| e.to_string()));
        match result {
            Ok(()) => self.reply(tab, Event::ok(req_id, path.display().to_string())),
            Err(e) => self.reply(tab, Event::err(req_id, format!("Export failed: {e}"))),
        }
    }
}

fn friendly_db_error(e: &DbError) -> String {
    match e {
        // Validation errors from core carry a readable message.
        DbError::InvalidParameterName(m) => m.clone(),
        DbError::QueryReturnedNoRows => "Not found".into(),
        _ => "Database error".into(),
    }
}
