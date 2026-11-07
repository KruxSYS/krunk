//! Downloads: the engine's download events, the downloads page and actions.

use super::{Browser, DOWNLOADS_KEY};
use crate::ffi::cef::{DownloadStart, DownloadStatus, DownloadUpdate, View};
use crate::ffi::dialogs;
use crate::log::{info, warning};
use lb_core::downloads::{interrupt_reason_text, sanitize_file_name, unique_path, DownloadState};
use lb_core::pages::Page;
use lb_core::protocol::{DownloadAction, Event, PopupContent, PopupKind, Request};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Minimum time between progress pushes to the downloads page.
const PROGRESS_PUSH_MS: i64 = 400;

/// Rows the downloads popup shows (the page shows everything).
const DOWNLOADS_POPUP_MAX: usize = 8;
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
    fn download_folder(&self) -> Option<PathBuf> {
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
        let ask = self.settings.borrow().ask_where_to_save;
        let name = sanitize_file_name(&start.suggested_name);
        let Some(folder) = self.download_folder() else {
            warning!("download {}: no download folder", start.id);
            self.abandon_download(start);
            return;
        };
        let mut path = unique_path(&folder, &name, |p| p.exists());
        if ask {
            let Some(w) = self.window() else { return };
            match dialogs::save_file(w.hwnd(), "Save as", &name, ("All files", "*.*"), Some(&folder)) {
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

    /// The downloads button's popup: the list, newest first.
    pub(super) fn downloads_content(&self) -> PopupContent {
        let mut items = self.downloads.borrow().items();
        items.reverse();
        items.truncate(DOWNLOADS_POPUP_MAX);
        PopupContent::Downloads { items }
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
                let items = self.downloads.borrow().items();
                self.reply(tab, Event::ok(req_id, items));
            }
            Request::ClearDownloads => {
                self.downloads.borrow_mut().clear_finished();
                self.save_downloads();
                self.reply(tab, Event::ok(req_id, ()));
                self.push_downloads();
            }
            Request::DownloadAction { id, action } => {
                let item = self.downloads.borrow().get(id).cloned();
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

    pub(super) fn save_downloads(&self) {
        let json = self.downloads.borrow().to_json();
        if let Ok(store) = self.store.try_borrow() {
            if let Err(e) = store.kv_set(DOWNLOADS_KEY, &json) {
                warning!("saving downloads: {e}");
            }
        }
    }

    /// Sends the list to every tab showing the downloads page, and the
    /// active count to the shell.
    pub(super) fn push_downloads(&self) {
        let (items, active) = {
            let d = self.downloads.borrow();
            (d.items(), d.active_count())
        };
        let event = Event::Downloads { items }.to_json();
        // The button's popup, if open, shows the same list.
        if self.popup_kind() == Some(PopupKind::Downloads) {
            self.show_popup(self.downloads_content(), None);
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
                c.post_json(&event);
            }
        }
        self.post_shell(&Event::DownloadsChanged { active });
    }
}
