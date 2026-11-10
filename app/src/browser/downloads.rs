//! Downloads: the engine's download events, the downloads page and actions.

use super::Browser;
use crate::ffi::cef::{DownloadStart, DownloadStatus, DownloadUpdate, View};
use crate::ffi::dialogs;
use crate::log::{info, warning};
use lb_core::downloads::{
    interrupt_reason_text, sanitize_file_name, unique_path, DownloadItem, DownloadPage, DownloadQuery, DownloadState,
};
use lb_core::pages::Page;
use lb_core::protocol::{DownloadAction, Event, PopupContent, PopupKind, Request};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Minimum time between progress pushes to the downloads page.
const PROGRESS_PUSH_MS: i64 = 400;

/// Rows the downloads popup shows (the page shows everything, in pages).
const DOWNLOADS_POPUP_MAX: u32 = 8;
/// What `GetDownloads` returns at most (0.15.0; the page uses `QueryDownloads`).
const GET_DOWNLOADS_MAX: u32 = 200;
/// The engine restarts the downloads of the last session within this long after the start.
const ENGINE_RESTART_GRACE_MS: i64 = 20_000;

impl Browser {
    /// `tab`: the tab whose view this is (a tab with a running download
    /// is never put to sleep).
    pub(super) fn hook_downloads(&self, c: &View, tab: Option<TabId>) {
        let weak = self.me.clone();
        let private = c.is_private();
        c.on_download_start(move |start| {
            if let Some(b) = weak.upgrade() {
                // The save dialog runs its own message loop: not inside the
                // engine's callback.
                b.defer(move |b| b.download_starting(start, private, tab));
            }
        });
        let weak = self.me.clone();
        let last_push = Rc::new(Cell::new(0i64));
        c.on_download_update(move |update| {
            if let Some(b) = weak.upgrade() {
                b.download_updated(update, tab, &last_push);
            }
        });
    }

    /// The folder downloads go to: the configured one, else the user's
    /// Downloads folder.
    pub(super) fn download_folder(&self) -> Option<PathBuf> {
        let dir = self.settings.borrow().download_dir.clone();
        if !dir.is_empty() && Path::new(&dir).is_dir() {
            return Some(PathBuf::from(dir));
        }
        dialogs::downloads_folder()
    }

    fn download_starting(&self, start: DownloadStart, private: bool, tab: Option<TabId>) {
        // Krunk's own list already has it (paused to resume, or interrupted with Retry); the
        // engine's copy would start it a second time (0.7.0). The engine restarts downloads it
        // still had at the last close (also ones Krunk took over: the cancel of a dropped
        // callback is not always saved) right after the start, as new ones.
        let restarted = {
            let (loaded, urls) = &*self.stopped_by_close.borrow();
            now_ms() - loaded < ENGINE_RESTART_GRACE_MS && urls.contains(&start.url)
        };
        if start.from_earlier_session || restarted {
            info!("engine download {} from an earlier session: not continued (Krunk's list has it)", start.id);
            self.abandon_download(start);
            return;
        }
        // A Web Store package (0.7.0, R20): never saved, it goes to the install flow.
        if let Some(id) = lb_core::ext::webstore_crx_id(&start.url) {
            info!("download of extension {id} from the Web Store: to the install flow");
            self.abandon_download(start);
            if !private {
                self.ext_install_from_store(&id, None, None, tab);
            }
            return;
        }
        // A PDF (0.11.3): shown in the viewer instead of saved, also when the site sends it as
        // a download; the viewer's own Download button saves it (a file: download, not this).
        let pdf = self.settings.borrow().pdf_in_browser
            && !start.url.starts_with("file:")
            && lb_core::downloads::is_pdf(&start.mime, &start.suggested_name);
        if let Some(tab) = tab.filter(|_| pdf) {
            return self.view_pdf(start, tab);
        }
        // "Save image as…" (0.12.2) always asks, with an image's file name.
        let save_image = self.take_save_image_as(&start.url, &start.original_url);
        let ask = self.settings.borrow().ask_where_to_save || save_image;
        let name = if save_image {
            sanitize_file_name(&lb_core::image_menu::file_name(&start.suggested_name, &start.url, &start.mime))
        } else {
            sanitize_file_name(&start.suggested_name)
        };
        let Some(folder) = self.download_folder() else {
            warning!("download {}: no download folder", start.id);
            self.abandon_download(start);
            return;
        };
        let mut path = unique_path(&folder, &name, |p| p.exists());
        if ask {
            let Some(w) = self.window() else { return };
            let title = if save_image { "Save image as" } else { "Save as" };
            match dialogs::save_file(w.hwnd(), title, &name, ("All files", "*.*"), Some(&folder)) {
                Some(p) => path = p,
                None => {
                    self.abandon_download(start);
                    return;
                }
            }
        }
        // Large files from servers that send parts: several connections (see segmented.rs).
        if let Some((start, path)) = self.try_parts(start, path, private, tab) {
            self.accept_download(start, path, private, tab);
        }
    }

    /// Fetches PDF `start` into Krunk's viewer folder; when it is complete it opens in the
    /// viewer (`pdf_fetched`). It is not in the downloads list.
    fn view_pdf(&self, start: DownloadStart, tab: TabId) {
        let mut name = sanitize_file_name(&start.suggested_name);
        if !name.to_ascii_lowercase().ends_with(".pdf") {
            name.push_str(".pdf");
        }
        // pdf-view/<n>/<name>: the viewer shows (and its Download button offers) the real name.
        // A private tab's PDFs go to pdf-view/private, deleted with its last private tab.
        let private = self.tab(tab).is_some_and(|t| t.private);
        let base = self.dirs.root().join("pdf-view");
        let base = if private { base.join("private") } else { base };
        let dir = base.join(format!("{}-{}", now_ms(), start.id));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            warning!("pdf {}: no viewer folder ({e}); downloading it instead", start.id);
            let Some(folder) = self.download_folder() else { return self.abandon_download(start) };
            let path = unique_path(&folder, &name, |p| p.exists());
            return self.accept_download(start, path, false, Some(tab));
        }
        let path = dir.join(&name);
        info!("pdf {}: fetching for the viewer", start.id);
        self.pdf_views.borrow_mut().insert(start.id, (tab, path.clone()));
        start.accept(&path);
    }

    /// A PDF for the viewer is complete (or failed): it opens in the tab that asked for it
    /// when that tab shows nothing yet, else in a new tab next to it.
    fn pdf_fetched(&self, engine_id: u32, status: DownloadStatus) {
        let Some((tab, path)) = self.pdf_views.borrow_mut().remove(&engine_id) else { return };
        if status != DownloadStatus::Completed {
            info!("pdf {engine_id}: not fetched ({status:?})");
            let _ = path.parent().map(std::fs::remove_dir_all);
            return;
        }
        let Ok(url) = url::Url::from_file_path(&path) else { return };
        info!("pdf {engine_id}: opened in the viewer");
        let blank = self.tab(tab).is_none_or(|t| t.url.is_empty() || t.url == "about:blank");
        if blank && self.tab(tab).is_some() {
            self.navigate_tab(tab, url.as_str());
        } else {
            self.open_tab_after(tab, url.as_str());
        }
    }

    /// Deletes the viewer's PDFs older than a few days (at start; a restored tab may still
    /// show a recent one).
    pub(super) fn clear_pdf_views(&self) {
        const KEEP: std::time::Duration = std::time::Duration::from_secs(3 * 24 * 3600);
        self.clear_private_pdfs();
        let Ok(dirs) = std::fs::read_dir(self.dirs.root().join("pdf-view")) else { return };
        for d in dirs.flatten() {
            let old = d
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > KEEP);
            if old {
                let _ = std::fs::remove_dir_all(d.path());
            }
        }
    }

    /// Deletes the private tabs' PDFs (their last tab closed, or at start).
    pub(super) fn clear_private_pdfs(&self) {
        let dir = self.dirs.root().join("pdf-view").join("private");
        if dir.is_dir() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// The engine downloads it (one connection).
    pub(super) fn accept_download(&self, start: DownloadStart, path: PathBuf, private: bool, tab: Option<TabId>) {
        if let Some(v) = tab.and_then(|t| self.views.borrow().get(&t).map(|v| v.downloads.clone())) {
            v.set(v.get() + 1);
        }
        let page = tab.and_then(|t| self.tab(t).map(|t| t.url.clone()));
        let id = {
            let mut d = self.downloads.borrow_mut();
            let id = d.start(&start.url, &path, start.total, now_ms(), private);
            if let Some(page) = page {
                d.set_page(id, &page);
            }
            id
        };
        info!("download {id} started");
        self.download_ops.borrow_mut().insert(start.id, (id, None));
        start.accept(&path);
        self.push_downloads();
        self.download_flight(tab);
    }

    /// A finished download is marked as from the Internet (its `Zone.Identifier` stream), so
    /// SmartScreen and Office's Protected View see it, unless the engine marked it already
    /// (0.11.7, audit C7: downloads fetched in parts had no mark).
    pub(super) fn mark_of_the_web(&self, id: u32) {
        let Some((path, text)) = self.downloads.borrow().get(id).map(|item| {
            (item.path.clone(), lb_core::downloads::zone_identifier(&item.url, item.page_url.as_deref(), item.private))
        }) else {
            return;
        };
        if !std::path::Path::new(&path).is_absolute() || !std::path::Path::new(&path).is_file() {
            return;
        }
        let stream = format!("{path}:Zone.Identifier");
        if std::path::Path::new(&stream).exists() {
            return;
        }
        // Not every file system has streams (FAT on a USB stick): then there is no mark, as in Chrome.
        if let Err(e) = std::fs::write(&stream, text) {
            warning!("download {id}: no mark of the web ({e})");
        }
    }

    /// The downloads button's popup: the list, newest first.
    pub(super) fn downloads_content(&self) -> PopupContent {
        PopupContent::Downloads { items: self.recent_downloads(DOWNLOADS_POPUP_MAX) }
    }

    /// The newest `limit` downloads (the popup, the agent).
    pub(super) fn recent_downloads(&self, limit: u32) -> Vec<DownloadItem> {
        self.query_downloads(&DownloadQuery { limit, ..Default::default() }).items
    }

    /// A page of the list (0.15.0): the table merged with what is in memory.
    fn query_downloads(&self, q: &DownloadQuery) -> DownloadPage {
        let d = self.downloads.borrow();
        let page = match self.store.try_borrow() {
            Ok(store) => store.downloads_query(&d, q).map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        page.unwrap_or_else(|e| {
            warning!("downloads query: {e}");
            DownloadPage { offset: q.offset, ..Default::default() }
        })
    }

    /// Download `id`, from memory or the table.
    fn download_item(&self, id: u32) -> Option<DownloadItem> {
        let d = self.downloads.borrow();
        self.store.try_borrow().ok().and_then(|s| s.download_get(&d, id).ok().flatten())
    }

    /// The list at start (0.15.0: from the table, storage V9): finished downloads older than
    /// Settings → Downloads' limit go; running ones were stopped by the close.
    pub(super) fn load_downloads(&self) {
        let days = self.settings.borrow().download_auto_clear_days;
        let loaded = self.store.borrow().downloads_load(now_ms(), days);
        match loaded {
            Ok(d) => *self.downloads.borrow_mut() = d,
            Err(e) => warning!("loading downloads: {e}"),
        }
        *self.stopped_by_close.borrow_mut() = (now_ms(), self.downloads.borrow().stopped_by_close());
        self.remove_leftover_parts();
        self.save_downloads();
    }

    /// Settings → Downloads' "Remove finished downloads from the list" changed (0.15.0): the
    /// ones older than the new limit go at once.
    pub(super) fn downloads_auto_clear(&self) {
        let days = self.settings.borrow().download_auto_clear_days;
        let cleared = self.store.borrow().downloads_clear_older(now_ms(), days);
        match cleared {
            Ok(0) => {}
            Ok(n) => {
                info!("downloads: {n} finished entries older than {days} days removed from the list");
                self.downloads.borrow_mut().mark_list_changed();
                self.push_downloads();
            }
            Err(e) => warning!("downloads auto-clear: {e}"),
        }
    }

    /// The engine's download is not wanted (Krunk fetches it in parts, or the save dialog was
    /// cancelled). Dropping its callback should cancel it, but the engine kept such downloads
    /// alive and resumed them at the next start (0.7.0): its next update cancels it for sure.
    pub(super) fn abandon_download(&self, start: DownloadStart) {
        self.abandoned_downloads.borrow_mut().insert(start.id);
        drop(start);
    }

    fn download_updated(&self, update: DownloadUpdate, tab: Option<TabId>, last_push: &Cell<i64>) {
        if self.abandoned_downloads.borrow().contains(&update.id) {
            match update.status {
                DownloadStatus::InProgress | DownloadStatus::Paused => {
                    info!("engine download {} given up: cancelled", update.id);
                    update.control.cancel();
                }
                _ => {
                    self.abandoned_downloads.borrow_mut().remove(&update.id);
                }
            }
            return;
        }
        if self.pdf_views.borrow().contains_key(&update.id) {
            if !matches!(update.status, DownloadStatus::InProgress | DownloadStatus::Paused) {
                self.pdf_fetched(update.id, update.status);
            }
            return;
        }
        // Updates before the download was accepted are not ours yet.
        let Some(id) = self.download_ops.borrow().get(&update.id).map(|(id, _)| *id) else { return };
        if let Some(op) = self.download_ops.borrow_mut().get_mut(&update.id) {
            op.1 = Some(update.control.clone());
        }
        let (state, reason) = match update.status {
            DownloadStatus::Completed => (DownloadState::Completed, None),
            DownloadStatus::Cancelled => (DownloadState::Cancelled, None),
            DownloadStatus::Interrupted(code) => match interrupt_reason_text(code) {
                None => (DownloadState::Cancelled, None),
                reason => (DownloadState::Interrupted, reason),
            },
            DownloadStatus::Paused => (DownloadState::Paused, None),
            DownloadStatus::InProgress => (DownloadState::InProgress, None),
        };
        let changed = {
            let mut d = self.downloads.borrow_mut();
            let before = d.get(id).map(|i| i.state);
            d.progress_at(id, update.received, update.total, now_ms());
            if !update.path.as_os_str().is_empty() {
                d.set_path(id, &update.path);
            }
            d.set_state(id, state, reason, now_ms());
            before != Some(state)
        };
        if state == DownloadState::Completed && changed {
            self.mark_of_the_web(id);
        }
        if state.is_finished() {
            if let Some(v) = tab.and_then(|t| self.views.borrow().get(&t).map(|v| v.downloads.clone())) {
                v.set(v.get().saturating_sub(1));
            }
            self.download_ops.borrow_mut().remove(&update.id);
            info!("download {id} finished: {state:?}");
        }
        let now = now_ms();
        if changed {
            self.save_downloads();
        }
        if changed || now - last_push.get() >= PROGRESS_PUSH_MS {
            last_push.set(now);
            self.push_downloads();
        }
    }

    /// A live tab to start a download in: the current window's active tab, else any tab, of the
    /// same kind (private or not).
    pub(super) fn download_view(&self, private: bool) -> Option<View> {
        let active = self.active_tab().filter(|t| t.private == private).map(|t| t.id);
        let others: Vec<TabId> =
            self.ws.borrow().tabs().filter(|(_, t)| t.private == private).map(|(_, t)| t.id).collect();
        let views = self.views.borrow();
        active.into_iter().chain(others).find_map(|id| views.get(&id).map(|v| v.view.clone()))
    }

    /// The engine's controls for our download `id`, if it is running.
    fn download_control(&self, id: u32) -> Option<crate::ffi::cef::DownloadControl> {
        self.download_ops.borrow().values().find(|(ours, _)| *ours == id).and_then(|(_, c)| c.clone())
    }

    pub(super) fn dispatch_downloads(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        match request {
            Request::GetDownloads => {
                let items = self.recent_downloads(GET_DOWNLOADS_MAX);
                self.reply(tab, Event::ok(req_id, items));
            }
            Request::QueryDownloads { text, filter, offset, limit, counts, since } => {
                let page = self.query_downloads(&DownloadQuery { text, filter, offset, limit, counts, since });
                self.reply(tab, Event::ok(req_id, page));
            }
            Request::ClearDownloads => {
                self.downloads.borrow_mut().clear_finished();
                self.save_downloads();
                self.reply(tab, Event::ok(req_id, ()));
                self.push_downloads();
            }
            Request::DownloadAction { id, action } => {
                let item = self.download_item(id);
                let Some(item) = item else {
                    self.reply(tab, Event::err(req_id, "Download not found"));
                    return;
                };
                if matches!(action, DownloadAction::Resume | DownloadAction::Retry) {
                    // The user asks for it: whatever the engine starts for it now is wanted.
                    self.stopped_by_close.borrow_mut().1.retain(|u| *u != item.url);
                }
                if matches!(action, DownloadAction::Pause | DownloadAction::Resume | DownloadAction::Cancel)
                    && self.parts_control(id, action)
                {
                    self.reply(tab, Event::ok(req_id, ()));
                    self.push_downloads();
                    return;
                }
                let op = self.download_control(id);
                let path = PathBuf::from(&item.path);
                match action {
                    DownloadAction::Open | DownloadAction::ShowInFolder => {
                        if item.state != DownloadState::Completed || !path.is_absolute() || !path.exists() {
                            self.reply(tab, Event::err(req_id, "The file no longer exists"));
                            return;
                        }
                        if action == DownloadAction::Open {
                            dialogs::open_path(&path);
                        } else {
                            dialogs::show_in_folder(&path);
                        }
                    }
                    DownloadAction::Cancel => {
                        if let Some(op) = op {
                            op.cancel();
                        }
                    }
                    DownloadAction::Pause => {
                        if let Some(op) = op {
                            op.pause();
                            self.downloads.borrow_mut().set_state(id, DownloadState::Paused, None, now_ms());
                        }
                    }
                    DownloadAction::Resume => {
                        if let Some(op) = op {
                            op.resume();
                            self.downloads.borrow_mut().set_state(id, DownloadState::InProgress, None, now_ms());
                        }
                    }
                    DownloadAction::Remove => {
                        self.downloads.borrow_mut().remove(id);
                        self.save_downloads();
                    }
                    DownloadAction::CopyLink | DownloadAction::CopyPageAddress => {
                        let text = if action == DownloadAction::CopyLink {
                            Some(item.url.clone())
                        } else {
                            item.page_url.clone()
                        };
                        let copied = match (text, self.window()) {
                            (Some(t), Some(w)) => crate::ffi::secure::copy_text(w.hwnd(), &t),
                            _ => false,
                        };
                        if !copied {
                            self.reply(tab, Event::err(req_id, "Could not copy the address"));
                            return;
                        }
                    }
                    DownloadAction::Retry => {
                        if !item.state.is_finished() {
                            self.reply(tab, Event::err(req_id, "The download is still running"));
                            return;
                        }
                        // Through a tab of the same kind: a private download stays private.
                        let Some(view) = self.download_view(item.private) else {
                            self.reply(tab, Event::err(req_id, "Open a private tab to download this again"));
                            return;
                        };
                        info!("download {id}: downloading again");
                        view.start_download(&item.url);
                    }
                    DownloadAction::DeleteFile => {
                        if item.state != DownloadState::Completed || !path.is_absolute() || !path.is_file() {
                            self.reply(tab, Event::err(req_id, "The file no longer exists"));
                            return;
                        }
                        if let Err(e) = std::fs::remove_file(&path) {
                            warning!("download {id}: deleting the file: {e}");
                            self.reply(tab, Event::err(req_id, "Could not delete the file"));
                            return;
                        }
                        info!("download {id}: file deleted");
                        self.downloads.borrow_mut().remove(id);
                        self.save_downloads();
                    }
                }
                self.reply(tab, Event::ok(req_id, ()));
                self.push_downloads();
            }
            _ => {}
        }
    }

    /// Writes what changed in the list to the table (0.15.0: rows of new and changed downloads;
    /// never progress alone).
    pub(super) fn save_downloads(&self) {
        if let Ok(store) = self.store.try_borrow() {
            if let Err(e) = store.downloads_save(&mut self.downloads.borrow_mut()) {
                warning!("saving downloads: {e}");
            }
        }
    }

    /// Tells every tab showing the downloads page what changed (0.15.0: the running downloads
    /// that moved, and a signal when the list itself changed, so the page fetches the rows it
    /// shows again), refreshes the button's popup, and sends the active count to the shell.
    pub(super) fn push_downloads(&self) {
        let (moved, changed, active) = {
            let mut d = self.downloads.borrow_mut();
            (d.take_moved(), d.take_list_changed(), d.active_count())
        };
        // The button's popup, if open, shows the newest ones.
        if self.popup_kind() == Some(PopupKind::Downloads) {
            self.show_popup(self.downloads_content(), None);
        }
        let mut events = Vec::new();
        if !moved.is_empty() {
            events.push(Event::Downloads { items: moved }.to_json());
        }
        if changed {
            events.push(Event::DownloadsChanged { active }.to_json());
        }
        if events.is_empty() {
            self.post_shell(&Event::DownloadsChanged { active });
            return;
        }
        let tabs: Vec<TabId> = self
            .ws
            .borrow()
            .tabs()
            .filter(|(_, t)| Page::from_url(&t.url) == Some(Page::Downloads))
            .map(|(_, t)| t.id)
            .collect();
        for id in tabs {
            if let Some(c) = self.tab_view(id) {
                for event in &events {
                    c.post_json(event);
                }
            }
        }
        self.post_shell(&Event::DownloadsChanged { active });
    }
}
