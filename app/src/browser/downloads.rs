//! Downloads: DownloadStarting, progress, the downloads page and actions.

use super::{Browser, DOWNLOADS_KEY};
use crate::ffi::dialogs;
use crate::ffi::webview::{Controller, DownloadStart, DownloadStatus};
use crate::log::{info, warning};
use lb_core::downloads::{sanitize_file_name, DownloadState};
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
    /// Uses the configured download folder (if any) for this WebView's profile.
    pub(super) fn apply_download_folder(&self, c: &Controller) {
        let dir = self.settings.borrow().download_dir.clone();
        if !dir.is_empty() && Path::new(&dir).is_dir() {
            c.set_download_folder(Path::new(&dir));
        }
    }

    pub(super) fn hook_downloads(&self, c: &Controller) {
        let weak = self.me.clone();
        let private = c.is_private();
        c.on_download_starting(move |start| {
            if let Some(b) = weak.upgrade() {
                b.download_starting(start, private);
            }
        });
    }

    fn download_starting(&self, start: DownloadStart, private: bool) {
        let ask = self.settings.borrow().ask_where_to_save;
        let mut path = start.suggested_path.clone();
        if ask {
            let name =
                sanitize_file_name(&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            let folder = path.parent().map(Path::to_path_buf);
            let Some(w) = self.window() else { return };
            match dialogs::save_file(w.hwnd(), "Save as", &name, ("All files", "*.*"), folder.as_deref()) {
                Some(p) => {
                    start.set_path(&p);
                    path = p;
                }
                None => {
                    start.cancel();
                    return;
                }
            }
        }
        let op = start.operation.clone();
        let id = self.downloads.borrow_mut().start(&start.uri, &path, start.total, now_ms(), private);
        info!("download {id} started");
        self.download_ops.borrow_mut().insert(id, op.clone());

        let last_push = Rc::new(Cell::new(0i64));
        let weak = self.me.clone();
        let op2 = op.clone();
        op.on_progress(move || {
            let Some(b) = weak.upgrade() else { return };
            b.downloads.borrow_mut().progress(id, op2.received(), op2.total());
            let now = now_ms();
            if now - last_push.get() >= PROGRESS_PUSH_MS {
                last_push.set(now);
                b.push_downloads();
            }
        });
        let weak = self.me.clone();
        let op3 = op.clone();
        op.on_state(move || {
            let Some(b) = weak.upgrade() else { return };
            let (state, reason) = match op3.status() {
                DownloadStatus::Completed => (DownloadState::Completed, None),
                DownloadStatus::Interrupted(code) => {
                    let cancelled = code == 26; // COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED
                    if cancelled {
                        (DownloadState::Cancelled, None)
                    } else {
                        (DownloadState::Interrupted, Some(interrupt_reason(code)))
                    }
                }
                DownloadStatus::InProgress => (DownloadState::InProgress, None),
            };
            {
                let mut d = b.downloads.borrow_mut();
                d.set_path(id, &op3.path());
                d.set_state(id, state, reason);
            }
            if state.is_finished() {
                b.download_ops.borrow_mut().remove(&id);
                info!("download {id} finished: {state:?}");
            }
            b.save_downloads();
            b.push_downloads();
        });
        drop(start);
        self.push_downloads();
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
                let op = self.download_ops.borrow().get(&id).cloned();
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
            if let Some(c) = self.controller(id) {
                c.post_json(&event);
            }
        }
        self.post_shell(&Event::DownloadsChanged { active });
    }
}

/// Readable text for a COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON code.
fn interrupt_reason(code: i32) -> String {
    match code {
        2 => "Access denied".into(),
        3 => "Disk full".into(),
        4 => "File name too long".into(),
        5 => "File too large".into(),
        6 => "Virus detected".into(),
        8 => "Blocked by policy".into(),
        9 => "Security check failed".into(),
        1 | 7 | 10 | 11 => "File error".into(),
        12..=16 => "Network error".into(),
        20 | 22 => "Not allowed by the server".into(),
        21 => "Certificate problem".into(),
        17..=25 => "Server error".into(),
        27 => "Browser closed".into(),
        29 => "Download process crashed".into(),
        _ => format!("Interrupted (code {code})"),
    }
}
