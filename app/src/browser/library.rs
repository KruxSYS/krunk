//! History, bookmarks, new-tab tiles and clearing browsing data.

use super::Browser;
use crate::ffi::cef::ClearKinds;
use crate::ffi::dialogs;
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

/// Collected page titles are written this often (`Browser::record_title`).
pub(super) const TITLE_TIMER: usize = 20;
const TITLE_FLUSH_MS: u32 = 2_000;

impl Browser {
    /// Requests of the built-in pages and the shell's library features.
    pub(super) fn dispatch_library(&self, sender: Sender, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        let now = now_ms();
        match request {
            Request::ToggleBookmark => self.toggle_bookmark(),
            Request::ShowBookmarkFolder { id, rect, from } => {
                // A click on the folder button that closed its dropdown stays closed.
                if rect.is_none() || !self.toggle_suppressed(&format!("folder:{id}")) {
                    self.show_bookmark_folder(id, rect, from);
                }
            }
            Request::BookmarkFolderBack => self.bookmark_folder_back(),
            Request::OpenBookmarkBackground { id } => {
                let url = self.store.borrow().bookmark(id).ok().flatten().and_then(|b| b.url);
                if let Some(url) = url {
                    if sender == Sender::Popup {
                        self.open_keeping_dropdown(&url);
                    } else {
                        self.open_in_background(&url);
                    }
                }
            }
            Request::Find { query, backwards } => self.find(&query, backwards),
            Request::FindClose => {
                let active = self.active_id();
                if let Some(id) = active {
                    self.stop_find(id);
                }
            }

            // ---- new tab page ----
            Request::GetTopSites => {
                let (source, n) = {
                    let s = self.settings.borrow();
                    (s.ntp.tiles, s.ntp.tile_count())
                };
                let r = match source {
                    lb_core::settings::TileSource::MostVisited => self.store.borrow().top_sites(n, now),
                    lb_core::settings::TileSource::Custom => self.store.borrow().pinned_tiles(n),
                };
                self.reply_db(tab, req_id, r)
            }
            Request::GetNewTabConfig => self.reply(tab, Event::ok(req_id, self.ntp_config())),
            Request::SetNtpOptions { ntp } => {
                // The image file name is the host's: the page cannot point it elsewhere.
                let image = self.settings.borrow().ntp.image.clone();
                let mut candidate = self.settings.borrow().clone();
                candidate.ntp = lb_core::settings::Ntp { image, ..ntp };
                match self.save_settings_from(tab, req_id, candidate) {
                    Ok(()) => self.reply(tab, Event::ok(req_id, self.ntp_config())),
                    Err(errors) => self.reply(tab, Event::err(req_id, errors.join("; "))),
                }
            }
            Request::NtpPickImage => self.ntp_pick_image(tab, req_id),
            Request::ReorderTiles { urls } => {
                let urls: Vec<String> = urls.into_iter().filter(|u| bookmarkable(u)).take(72).collect();
                self.reply_db(tab, req_id, self.store.borrow().reorder_tiles(&urls))
            }
            Request::AddTile { url, title } => {
                let u = urlclean::input_to_url(url.trim(), self.settings.borrow().search_template());
                if !bookmarkable(&u) {
                    return self.reply(tab, Event::err(req_id, "Enter a web address"));
                }
                let t = if title.trim().is_empty() {
                    urlclean::host_of(&u).unwrap_or_default()
                } else {
                    title.trim().to_string()
                };
                self.reply_db(tab, req_id, self.store.borrow().pin_site(&u, &t))
            }
            Request::EditTile { url, new_url, title } => {
                let u = urlclean::input_to_url(new_url.trim(), self.settings.borrow().search_template());
                if !bookmarkable(&u) {
                    return self.reply(tab, Event::err(req_id, "Enter a web address"));
                }
                self.reply_db(tab, req_id, self.store.borrow().edit_tile(&url, &u, title.trim()))
            }
            Request::PinSite { url, title } => self.reply_db(tab, req_id, self.store.borrow().pin_site(&url, &title)),
            Request::UnpinSite { url } => self.reply_db(tab, req_id, self.store.borrow().unpin_site(&url)),
            Request::HideSite { url } => self.reply_db(tab, req_id, self.store.borrow().hide_site(&url)),

            // ---- history ----
            Request::QueryHistory { text, before, limit, device } => {
                let filter = history::HistoryFilter::parse(&device);
                self.reply_db(tab, req_id, self.store.borrow().query_history_in(&text, before, limit, &filter))
            }
            Request::DeleteHistory { ids } => self.reply_db(tab, req_id, self.store.borrow_mut().delete_visits(&ids)),
            Request::DeleteHistoryRange { from, to, device } => {
                let filter = history::HistoryFilter::parse(&device);
                self.reply_db(tab, req_id, self.store.borrow_mut().delete_history_range_in(from, to, &filter))
            }
            Request::HistoryDevices => self.reply_db(tab, req_id, self.store.borrow().history_devices()),
            Request::ClearBrowsingData { data } => self.clear_browsing_data(tab, req_id, data),

            // ---- bookmarks ----
            Request::GetBookmarks => self.reply_db(tab, req_id, self.store.borrow().bookmark_tree()),
            Request::SearchBookmarks { text } => {
                self.reply_db(tab, req_id, self.store.borrow().search_bookmarks(&text))
            }
            Request::AddBookmark { parent, title, url } => {
                let r = self.store.borrow().add_bookmark(parent, &title, &url);
                if r.is_ok() {
                    // krunk://bookmarks never runs in a private window.
                    self.favicon_bookmark_added(&url, false);
                }
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
            r @ (Request::GetDownloads
            | Request::QueryDownloads { .. }
            | Request::ClearDownloads
            | Request::DownloadAction { .. }) => self.dispatch_downloads(tab, req_id, r),

            other => self.not_available(sender, tab, req_id, &other),
        }
    }

    /// Replies with a database result (errors become an error reply).
    /// What the new tab page needs: the search engine's name, the shortcut list and its options.
    pub(super) fn ntp_config(&self) -> serde_json::Value {
        let s = self.settings.borrow();
        let wallpaper = (s.ntp.background == lb_core::settings::NtpBackground::Wallpaper)
            .then(|| lb_core::assets::wallpaper(&s.ntp.wallpaper))
            .flatten();
        serde_json::json!({
            "searchName": s.search_name(),
            "shortcuts": lb_core::keys::SHORTCUTS,
            "ntp": s.ntp,
            "imageUrl": if s.ntp.image.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(lb_core::assets::ntp_image_url(&s.ntp.image)) },
            // 0.7.0 (R2): the built-in wallpapers and the chosen one.
            "wallpapers": wallpaper_list(),
            "wallpaperUrl": wallpaper.as_ref().map(|w| lb_core::assets::wallpaper_url(&w.id)),
            "wallpaperLook": wallpaper.as_ref().map(|w| w.look.clone()),
        })
    }

    /// Every open new tab page gets the options again (the customize panel of one page, or
    /// the settings page, changed them).
    pub(super) fn push_ntp_config(&self) {
        let event = Event::NewTabConfig { config: self.ntp_config() }.to_json();
        let tabs: Vec<TabId> = self
            .ws
            .borrow()
            .tabs()
            .filter(|(_, t)| lb_core::pages::Page::from_url(&t.url) == Some(lb_core::pages::Page::NewTab))
            .map(|(_, t)| t.id)
            .collect();
        for id in tabs {
            if let Some(c) = self.tab_view(id) {
                c.post_json(&event);
            }
        }
    }

    /// "Choose image": the file dialog, then a copy of the picture into the profile (the
    /// previous one is removed) and the setting switched to the image.
    fn ntp_pick_image(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let Some(w) = self.window() else { return };
        let Some(path) =
            dialogs::open_file(w.hwnd(), "Background image", ("Images", "*.png;*.jpg;*.jpeg;*.webp;*.gif"))
        else {
            return self.reply(tab, Event::err(req_id, "No image chosen"));
        };
        let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif") {
            return self.reply(tab, Event::err(req_id, "Choose a PNG, JPEG, WebP or GIF file"));
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                warning!("new tab background: reading {} failed: {e}", path.display());
                return self.reply(tab, Event::err(req_id, "The image could not be copied into the profile"));
            }
        };
        match self.store_ntp_image(&bytes, &ext) {
            Ok(()) => self.reply(tab, Event::ok(req_id, self.ntp_config())),
            Err(e) => self.reply(tab, Event::err(req_id, e)),
        }
    }

    /// Stores `bytes` as the new tab page's own image (`ntp-background-<ms>.<ext>` in the
    /// profile), switches the background to it and removes the previous file. Shared by
    /// "Choose image…" and the agent's `set_new_tab_background_url` (0.15.0).
    pub(super) fn store_ntp_image(&self, bytes: &[u8], ext: &str) -> Result<(), String> {
        let name = format!("ntp-background-{}.{ext}", now_ms());
        let dest = self.dirs.root().join(&name);
        if let Err(e) = std::fs::write(&dest, bytes) {
            warning!("new tab background: writing the image failed: {e}");
            return Err("The image could not be copied into the profile".into());
        }
        let old = self.settings.borrow().ntp.image.clone();
        let mut candidate = self.settings.borrow().clone();
        candidate.ntp.image = name;
        candidate.ntp.background = lb_core::settings::NtpBackground::Image;
        match self.save_settings_from(None, None, candidate) {
            Ok(()) => {
                if !old.is_empty() {
                    let _ = std::fs::remove_file(self.dirs.root().join(old));
                }
                info!("new tab background: image set");
                Ok(())
            }
            Err(errors) => {
                let _ = std::fs::remove_file(&dest);
                Err(errors.join("; "))
            }
        }
    }

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
    /// 0.26.0 (F-writes): the visit is written 1 s after the load ends, off the engine's
    /// load-end handling (it then also has the page's final title). Measurement builds:
    /// KRUNK_PERF_OFF=defer-writes writes it at once, as in 0.25.5.
    pub(super) fn record_visit_soon(&self, id: TabId) {
        #[cfg(feature = "perf-probe")]
        if super::perf_probe::off("defer-writes") {
            self.record_visit(id);
            return;
        }
        // The address now: a tab closed or gone elsewhere within the second keeps this visit.
        let Some((url, title, private)) = self.tab(id).map(|t| (t.url.clone(), t.title.clone(), t.private)) else {
            return;
        };
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(1000, move || {
            let Some(b) = weak.upgrade() else { return };
            let title = b.tab(id).filter(|t| t.url == url).map_or(title, |t| t.title.clone());
            b.write_visit(id, &url, &title, private);
        });
    }

    pub(super) fn record_visit(&self, id: TabId) {
        let (url, title, private) = {
            let Some(t) = self.tab(id) else { return };
            (t.url.clone(), t.title.clone(), t.private)
        };
        self.write_visit(id, &url, &title, private);
    }

    fn write_visit(&self, id: TabId, url: &str, title: &str, private: bool) {
        if let Some(clean) = history::visit_url(url, private) {
            if let Ok(store) = self.store.try_borrow() {
                // On the UI thread: the load trace shows what the write costs a navigation.
                let _s = crate::perf::LoadSpan::step(id, "history_visit");
                if let Err(e) = store.record_visit(&clean, title, now_ms()) {
                    warning!("history: {e}");
                }
            }
            let _s = crate::perf::LoadSpan::step(id, "sessionlog_visit");
            self.sessionlog_visit(id, &clean, title);
        }
    }

    /// The page title changed: update the stored history title. Titles are collected and
    /// written every 2 s on a worker (0.10.0): pages that count something in their title
    /// ("(3) Inbox") wrote to the database on the UI thread with every change.
    pub(super) fn record_title(&self, id: TabId) {
        let _s = crate::perf::LoadSpan::step(id, "history_title");
        let (url, title, private) = {
            let Some(t) = self.tab(id) else { return };
            (t.url.clone(), t.title.clone(), t.private)
        };
        if let Some(clean) = history::visit_url(&url, private) {
            let mut pending = self.pending_titles.borrow_mut();
            if pending.is_empty() {
                if let Some(w) = self.app() {
                    w.set_timer(TITLE_TIMER, TITLE_FLUSH_MS);
                }
            }
            pending.insert(clean, title);
        }
    }

    /// `TITLE_TIMER`: the collected titles go to the database on a worker connection.
    pub(super) fn flush_titles(&self) {
        if let Some(w) = self.app() {
            w.kill_timer(TITLE_TIMER);
        }
        let titles: Vec<(String, String)> =
            std::mem::take(&mut *self.pending_titles.borrow_mut()).into_iter().collect();
        if titles.is_empty() {
            return;
        }
        let db = self.dirs.database();
        self.spawn_job(
            "history-titles",
            move || lb_core::storage::Store::open(&db).and_then(|mut s| s.set_titles(&titles)),
            |_, r| {
                if let Err(e) = r {
                    warning!("history titles: {e}");
                }
            },
        );
    }

    /// At exit: what is still waiting, on this thread.
    pub(super) fn flush_titles_now(&self) {
        let titles: Vec<(String, String)> =
            std::mem::take(&mut *self.pending_titles.borrow_mut()).into_iter().collect();
        if let (false, Ok(mut store)) = (titles.is_empty(), self.store.try_borrow_mut()) {
            if let Err(e) = store.set_titles(&titles) {
                warning!("history titles: {e}");
            }
        }
    }

    /// Keeps the favicon for the new-tab page and bookmarks (not for private tabs).
    pub(super) fn cache_favicon(&self, id: TabId, png: &[u8]) {
        let (url, private) = {
            let Some(t) = self.tab(id) else { return };
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
        // Sites whose storage to clear, before their history goes.
        let origins = self.origins_to_clear(data.site_data, data.since);
        if data.history {
            let r = match data.since {
                Some(since) => self.store.borrow_mut().delete_history_range(since, now + 1).map(|_| ()),
                None => self.store.borrow_mut().clear_history(),
            };
            if let Err(e) = r {
                warning!("clear history: {e}");
            }
            // 0.26.0: what the engine's loading predictor learned from the visits goes too
            // (all of it, also for a time range).
            crate::ffi::cef::clear_predictor();
            if data.since.is_none() {
                let _ = self.store.borrow().vacuum();
            }
        }
        if data.downloads {
            self.downloads.borrow_mut().clear_finished();
            self.save_downloads();
            self.push_downloads();
        }
        if data.site_data && data.since.is_none() {
            // Stored permission answers are site data too.
            if let Err(e) = self.store.borrow().permissions_clear() {
                warning!("clear permissions: {e}");
            }
        }
        let kinds = ClearKinds { cookies: data.cookies, cache: data.cache, site_data: data.site_data };
        info!(
            "clearing browsing data (history {}, cookies {}, cache {}, site data {})",
            data.history, data.cookies, data.cache, data.site_data
        );
        let weak = self.me.clone();
        self.clear_engine_data(kinds, data.since, origins, move |ok| {
            if let Some(b) = weak.upgrade() {
                b.reply(tab, if ok { Event::ok(req_id, ()) } else { Event::err(req_id, "Clearing site data failed") });
            }
        });
    }

    /// Origins visited (in the range) whose site data is cleared.
    pub(super) fn origins_to_clear(&self, site_data: bool, since: Option<i64>) -> Vec<String> {
        if !site_data {
            return Vec::new();
        }
        self.store.borrow().visited_origins(since).unwrap_or_default()
    }

    /// Cookies, cache and the site data of `origins` in the default profile.
    pub(super) fn clear_engine_data(
        &self,
        kinds: ClearKinds,
        since: Option<i64>,
        origins: Vec<String>,
        done: impl FnOnce(bool) + 'static,
    ) {
        // The shell uses the default profile, so its DevTools session can
        // clear the profile's site data.
        let shell = self.shell();
        crate::ffi::cef::clear_profile_data(kinds, since, &origins, shell.as_ref(), done);
    }

    // ------------------------------------------------------------ bookmarks

    /// Star button / Ctrl+D: bookmark the active page in the bar, or remove it.
    pub(super) fn toggle_bookmark(&self) {
        let (url, title) = match self.active_tab() {
            Some(t) => (t.url.clone(), t.title.clone()),
            None => return,
        };
        if !bookmarkable(&url) {
            return;
        }
        let private = self.active_tab().is_some_and(|t| t.private);
        let r = self.store.borrow_mut().toggle_bookmark(&url, &title);
        match r {
            Ok(true) => self.favicon_bookmark_added(&url, private),
            Ok(false) => {}
            Err(e) => warning!("bookmark: {e}"),
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
    /// `from`: the bar's overflow (0.7.2, R10), its items from that index on. A refresh of the
    /// open dropdown (`rect` none, the same folder) keeps its offset; a subfolder has none.
    pub(super) fn show_bookmark_folder(&self, id: i64, rect: Option<PopupRect>, from: Option<u32>) {
        self.show_folder_level(id, rect, from, None);
    }

    /// The height of the screen's work area below `anchor` (shell CSS px of the current window).
    pub(super) fn room_below_css(&self, anchor: PopupRect) -> f64 {
        let Some(main) = self.window() else { return 600.0 };
        let scale = main.scale().max(0.5);
        let origin = main.client_to_screen(0, 0);
        let bottom = origin.1 + ((anchor.y + anchor.height + 2.0) * scale).round() as i32;
        let work = crate::ffi::window::Window::work_area_at(origin.0 + (anchor.x * scale).floor() as i32, bottom);
        f64::from((work.y + work.height - bottom).max(0)) / scale - 8.0
    }

    /// Middle-click in a dropdown (0.11.0): the new tab opens behind the current one and the
    /// dropdown stays open. Creating the tab's view takes the activation from the popup (on
    /// Windows), which would close it: that loss is ignored for a moment, and the dropdown
    /// gets the activation back.
    fn open_keeping_dropdown(&self, url: &str) {
        self.popup_hold_until.set(now_ms() + 600);
        self.open_in_background(url);
        self.refocus_dropdown();
    }

    /// A tab opened behind an open dropdown took the activation from it: it gets it back
    /// (bookmark folders 0.11.0; history menus 0.18.0).
    pub(super) fn refocus_dropdown(&self) {
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(120, move || {
            let Some(b) = weak.upgrade() else { return };
            let popup = b
                .popup
                .try_borrow()
                .ok()
                .and_then(|p| p.as_ref().filter(|p| p.open()).map(|p| (p.window, p.view.clone())));
            if let Some((w, view)) = popup {
                let r = w.rect();
                w.show_at(crate::ffi::window::ScreenRect { x: r.x, y: r.y, width: r.width, height: r.height }, true);
                if let Some(c) = view {
                    c.focus();
                }
            }
        });
    }

    /// "‹ Back" (or Backspace/Left) in a dropdown: the folder the open subfolder came from,
    /// with the overflow offset it had (0.11.0).
    fn bookmark_folder_back(&self) {
        if self.open_folder().is_none() {
            return;
        }
        let Some((id, offset)) = self.folder_stack.borrow_mut().pop() else { return };
        self.show_folder_level(id, None, None, Some(offset));
    }

    /// `back`: going up to a folder of the stack, with its offset.
    fn show_folder_level(&self, id: i64, rect: Option<PopupRect>, from: Option<u32>, back: Option<u32>) {
        let Ok(Some(folder)) = self.store.borrow().bookmark(id) else { return };
        if !folder.is_folder {
            return;
        }
        let open = self.open_folder();
        let offset = match (rect, from, back) {
            (_, _, Some(offset)) => offset,
            (Some(_), Some(from), _) if id == lb_core::storage::BOOKMARKS_BAR => from,
            (Some(_), _, _) => 0,
            (None, _, _) if open == Some(id) => self.folder_offset.get(),
            (None, _, _) => 0,
        };
        // The levels a dropdown went down through: a new dropdown starts over, a subfolder
        // opened in place remembers where it came from.
        match (rect, open) {
            (Some(_), _) => self.folder_stack.borrow_mut().clear(),
            (None, Some(parent)) if back.is_none() && parent != id => {
                self.folder_stack.borrow_mut().push((parent, self.folder_offset.get()));
            }
            _ => {}
        }
        let mut items = self.store.borrow().bar_items(id).unwrap_or_default();
        items.drain(..(offset as usize).min(items.len()));
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
        self.folder_offset.set(offset);
        let parent = self.folder_stack.borrow().last().map(|&(p, _)| p);
        let back = parent.and_then(|p| self.store.borrow().bookmark(p).ok().flatten()).map(|f| f.title);
        // The Back row is one more row.
        let rows = items.len() + usize::from(back.is_some());
        let popup_rect = layout::folder_popup_rect(anchor, rows, self.room_below_css(anchor));
        self.show_popup(
            PopupContent::BookmarkFolder { id, title: folder.title, items, offset, back },
            Some(popup_rect),
        );
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

/// The built-in wallpapers for the pickers (new tab page, settings).
pub(super) fn wallpaper_list() -> Vec<serde_json::Value> {
    lb_core::assets::wallpapers()
        .iter()
        .map(|w| {
            serde_json::json!({
                "id": w.id, "title": w.title, "look": w.look, "dim": w.dim,
                "thumb": lb_core::assets::wallpaper_thumb_url(&w.id),
            })
        })
        .collect()
}
