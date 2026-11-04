//! Downloads: the engine's download events, the downloads page and actions.

use super::{Browser, DOWNLOADS_KEY};
use crate::ffi::cef::{DownloadStart, DownloadStatus, DownloadUpdate, View};
use crate::ffi::dialogs;
use crate::log::{info, warning};
use lb_core::downloads::{interrupt_reason_text, sanitize_file_name, unique_path, DownloadState};
use lb_core::pages::Page;
use lb_core::protocol::{DownloadAction, Event, Request};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Minimum time between progress pushes to the downloads page.
const PROGRESS_PUSH_MS: i64 = 400;

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
        let ask = self.settings.borrow().ask_where_to_save;
        let name = sanitize_file_name(&start.suggested_name);
        let Some(folder) = self.download_folder() else {
            warning!("download {}: no download folder", start.id);
            return;
        };
        let mut path = unique_path(&folder, &name, |p| p.exists());
        if ask {
            let Some(w) = self.window() else { return };
            match dialogs::save_file(w.hwnd(), "Save as", &name, ("All files", "*.*"), Some(&folder)) {
                Some(p) => path = p,
                // Dropping `start` cancels the download.
                None => return,
            }
        }
        if let Some(v) = tab.and_then(|t| self.views.borrow().get(&t).map(|v| v.downloads.clone())) {
            v.set(v.get() + 1);
        }
        let id = self.downloads.borrow_mut().start(&start.url, &path, start.total, now_ms(), private);
        info!("download {id} started");
        self.download_ops.borrow_mut().insert(start.id, (id, None));
        start.accept(&path);
        self.push_downloads();
    }

    fn download_updated(&self, update: DownloadUpdate, tab: Option<TabId>, last_push: &Cell<i64>) {
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
            d.progress(id, update.received, update.total);
            if !update.path.as_os_str().is_empty() {
                d.set_path(id, &update.path);
            }
            d.set_state(id, state, reason);
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
                            self.downloads.borrow_mut().set_state(id, DownloadState::Paused, None);
                        }
                    }
                    DownloadAction::Resume => {
                        if let Some(op) = op {
                            op.resume();
                            self.downloads.borrow_mut().set_state(id, DownloadState::InProgress, None);
                        }
                    }
                    DownloadAction::Remove => {
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
        let tabs: Vec<TabId> = self
            .model
            .borrow()
            .tabs()
            .iter()
            .filter(|t| Page::from_url(&t.url) == Some(Page::Downloads))
            .map(|t| t.id)
            .collect();
        for id in tabs {
            if let Some(c) = self.tab_view(id) {
                c.post_json(&event);
            }
        }
        self.post_shell(&Event::DownloadsChanged { active });
    }
}
