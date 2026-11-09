//! The bookmarks bar (0.4.0): where it shows (always, only on the new-tab page, never), its
//! context menu (on a bookmark, a folder or the bar itself), the small editor for names and
//! addresses, and moves from dragging items on the bar or onto its folders.

use super::Browser;
use super::PopupSlot;
use crate::log::{info, warning};
use lb_core::bookmarks::bookmarkable;
use lb_core::layout;
use lb_core::pages::Page;
use lb_core::protocol::{BookmarkAction, PopupContent, PopupRect};
use lb_core::settings::BookmarksBar;
use lb_core::storage::{now_ms, BOOKMARKS_BAR};
use lb_core::tabs::Position;
use lb_core::windows::WindowId;

/// "Open all" opens at most this many bookmarks of a folder.
const OPEN_ALL_MAX: usize = 100;

impl Browser {
    /// Does window `win` show the bookmarks bar now?
    pub(super) fn bar_shown(&self, win: WindowId) -> bool {
        match self.settings.borrow().bookmarks_bar {
            BookmarksBar::Always => true,
            BookmarksBar::Never => false,
            BookmarksBar::NewTab => self
                .ws
                .try_borrow()
                .ok()
                .and_then(|ws| {
                    ws.model(win).and_then(|m| m.active().map(|t| Page::from_url(&t.url) == Some(Page::NewTab)))
                })
                .unwrap_or(false),
        }
    }

    /// Ctrl+Shift+B: a hidden bar comes back (always shown); a shown one ("always" or "only
    /// on new tabs") is hidden.
    pub(super) fn toggle_bookmarks_bar(&self) {
        let next = match self.settings.borrow().bookmarks_bar {
            BookmarksBar::Never => BookmarksBar::Always,
            BookmarksBar::Always | BookmarksBar::NewTab => BookmarksBar::Never,
        };
        info!("bookmarks bar: {next:?} (Ctrl+Shift+B)");
        self.change_settings(|s| s.bookmarks_bar = next);
    }

    /// Right-click on the bar: the menu for bookmark or folder `id`, or for the bar.
    pub(super) fn show_bookmark_menu(&self, id: Option<i64>, rect: PopupRect) {
        if !rect.is_valid() {
            return;
        }
        let (title, url, is_folder, children) = match id {
            Some(id) => {
                let Ok(Some(b)) = self.store.borrow().bookmark(id) else { return };
                if lb_core::bookmarks::is_root(id) {
                    return;
                }
                let children = if b.is_folder {
                    self.store.borrow().children(id).map(|c| c.iter().filter(|c| !c.is_folder).count()).unwrap_or(0)
                } else {
                    0
                };
                (b.title, b.url, b.is_folder, children as u32)
            }
            None => (String::new(), None, false, 0),
        };
        let can_add = self.active_tab().is_some_and(|t| bookmarkable(&t.url));
        let mode = self.settings.borrow().bookmarks_bar;
        self.bookmark_menu_at.set(Some(rect));
        let at = layout::bookmark_menu_rect(rect.x, rect.y, id.is_some(), is_folder);
        // From inside a folder dropdown the menu goes into the second popup window, so
        // the dropdown stays open underneath.
        let slot = if self.menu_from_folder.get().is_some() { PopupSlot::Menu } else { PopupSlot::Main };
        self.show_popup_in(
            slot,
            PopupContent::BookmarkMenu { id, is_folder, title, url, children, mode, can_add },
            Some(at),
        );
    }

    /// The popup the bookmark menu (or its editor) is in.
    fn bookmark_menu_slot(&self) -> PopupSlot {
        if self.menu_from_folder.get().is_some() {
            PopupSlot::Menu
        } else {
            PopupSlot::Main
        }
    }

    /// After a change made from a folder item's menu: the menu goes away and the dropdown
    /// shows the folder as it is now (or closes when the folder itself is gone).
    fn refresh_folder_after_menu(&self) {
        let Some(folder) = self.menu_from_folder.get() else { return };
        self.hide_menu_popup(false);
        self.menu_from_folder.set(None);
        let exists = matches!(self.store.borrow().bookmark(folder), Ok(Some(b)) if b.is_folder);
        if exists && self.open_folder() == Some(folder) {
            self.show_bookmark_folder(folder, None, None);
            if let Some(c) = self.popup.try_borrow().ok().and_then(|p| p.as_ref().and_then(|p| p.view.clone())) {
                c.focus();
            }
        } else {
            self.hide_popup();
        }
    }

    /// A choice in the bar's menu.
    pub(super) fn bookmark_menu_action(&self, id: Option<i64>, action: BookmarkAction) {
        let item = id.and_then(|id| self.store.borrow().bookmark(id).ok().flatten());
        // Where new things go: into the folder clicked, next to the bookmark clicked, else
        // onto the bar.
        let parent = match &item {
            Some(b) if b.is_folder => b.id,
            Some(b) => b.parent.unwrap_or(BOOKMARKS_BAR),
            None => BOOKMARKS_BAR,
        };
        let url = item.as_ref().and_then(|b| b.url.clone()).filter(|u| bookmarkable(u));
        match action {
            BookmarkAction::Edit | BookmarkAction::NewFolder => {
                let content = match (action, &item) {
                    (BookmarkAction::Edit, Some(b)) => PopupContent::BookmarkEdit {
                        id: Some(b.id),
                        parent: b.parent.unwrap_or(BOOKMARKS_BAR),
                        is_folder: b.is_folder,
                        title: b.title.clone(),
                        url: b.url.clone(),
                    },
                    (BookmarkAction::Edit, None) => return self.hide_popup(),
                    _ => PopupContent::BookmarkEdit {
                        id: None,
                        parent,
                        is_folder: true,
                        title: "New folder".into(),
                        url: None,
                    },
                };
                let at = self.bookmark_menu_at.get().unwrap_or(PopupRect { x: 8.0, y: 8.0, width: 1.0, height: 1.0 });
                let is_folder = matches!(content, PopupContent::BookmarkEdit { is_folder: true, .. });
                let slot = self.bookmark_menu_slot();
                self.show_popup_in(slot, content, Some(layout::bookmark_edit_rect(at.x, at.y, is_folder)));
                return;
            }
            // Opening something closes the dropdown too; editing keeps it (refreshed below).
            BookmarkAction::Delete | BookmarkAction::AddPage if self.menu_from_folder.get().is_some() => {}
            _ => self.hide_popup(),
        }
        let from_folder = self.menu_from_folder.get().is_some();
        match action {
            BookmarkAction::OpenNewTab => {
                if let Some(u) = url {
                    self.open_in_background(&u);
                }
            }
            BookmarkAction::OpenNewWindow => {
                if let Some(u) = url {
                    let bounds = self.cascade_bounds();
                    let (_, _, ops) = self.ws.borrow_mut().open_window(&u, false, bounds, false, now_ms());
                    self.apply(ops);
                }
            }
            BookmarkAction::OpenPrivate => {
                if let Some(u) = url {
                    self.new_tab(Some(&u), true);
                }
            }
            BookmarkAction::OpenAll => {
                let Some(b) = item.filter(|b| b.is_folder) else { return };
                let urls: Vec<String> = self
                    .store
                    .borrow()
                    .children(b.id)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|c| c.url)
                    .filter(|u| bookmarkable(u))
                    .take(OPEN_ALL_MAX)
                    .collect();
                info!("bookmarks bar: opening {} bookmarks of a folder", urls.len());
                for u in urls {
                    self.open_in_background(&u);
                }
            }
            BookmarkAction::Delete => {
                if let Some(b) = item {
                    let r = self.store.borrow_mut().delete_bookmark(b.id);
                    self.bar_changed("delete", r);
                }
            }
            BookmarkAction::AddPage => {
                let page = self.active_tab().map(|t| (t.url.clone(), t.title.clone()));
                if let Some((u, t)) = page.filter(|(u, _)| bookmarkable(u)) {
                    let r = self.store.borrow().add_bookmark(parent, &t, &u);
                    self.bar_changed("add", r);
                }
            }
            BookmarkAction::ShowAlways => self.change_settings(|s| s.bookmarks_bar = BookmarksBar::Always),
            BookmarkAction::ShowOnNewTab => self.change_settings(|s| s.bookmarks_bar = BookmarksBar::NewTab),
            BookmarkAction::ShowNever => self.change_settings(|s| s.bookmarks_bar = BookmarksBar::Never),
            BookmarkAction::Edit | BookmarkAction::NewFolder => {}
        }
        if from_folder {
            self.refresh_folder_after_menu();
        }
    }

    /// "Bookmark all tabs" (the title bar's menu): every bookmarkable tab of the current window
    /// goes into a new folder on the bar, named after the day.
    pub(super) fn bookmark_all_tabs(&self) {
        let Some(win) = self.cur_id() else { return };
        let pages: Vec<(String, String)> = {
            let ws = self.ws.borrow();
            let Some(m) = ws.model(win) else { return };
            m.tabs()
                .iter()
                .filter(|t| !t.private && bookmarkable(&t.url))
                .map(|t| (t.title.clone(), t.url.clone()))
                .collect()
        };
        if pages.is_empty() {
            return;
        }
        let name = format!("Tabs {}", lb_core::storage::date_ymd(now_ms()));
        let folder = match self.store.borrow().add_folder(BOOKMARKS_BAR, &name) {
            Ok(id) => id,
            Err(e) => return self.bar_changed("bookmark all tabs", Err::<(), _>(e)),
        };
        let mut r = Ok(());
        for (title, url) in &pages {
            let t = if title.trim().is_empty() { url.clone() } else { title.clone() };
            if let Err(e) = self.store.borrow().add_bookmark(folder, &t, url) {
                r = Err(e);
            }
        }
        info!("bookmarks bar: {} tabs bookmarked into '{name}'", pages.len());
        self.bar_changed("bookmark all tabs", r);
    }

    /// The editor's Save: rename or edit bookmark `id`, or a new folder in `parent`.
    pub(super) fn save_bookmark(&self, id: Option<i64>, parent: i64, title: &str, url: Option<&str>) {
        let from_folder = self.menu_from_folder.get().is_some();
        if !from_folder {
            self.hide_popup();
        }
        let r = match id {
            Some(id) => self.store.borrow().edit_bookmark(id, title, url),
            None => self.store.borrow().add_folder(parent, title).map(|_| ()),
        };
        self.bar_changed("edit", r);
        if from_folder {
            self.refresh_folder_after_menu();
        }
    }

    /// The open folder dropdown's items: is `id` one of them (`folders_too`: or a subfolder)?
    pub(super) fn folder_dropdown_has(&self, id: i64, folders_too: bool) -> bool {
        let Ok(popup) = self.popup.try_borrow() else { return false };
        let Some(PopupContent::BookmarkFolder { items, .. }) =
            popup.as_ref().filter(|p| p.open()).and_then(|p| p.content.as_ref())
        else {
            return false;
        };
        items.iter().any(|i| i.id == id && (folders_too || !i.is_folder))
    }

    /// The open folder dropdown's folder id.
    pub(super) fn open_folder(&self) -> Option<i64> {
        let popup = self.popup.try_borrow().ok()?;
        match popup.as_ref().filter(|p| p.open())?.content.as_ref()? {
            PopupContent::BookmarkFolder { id, .. } => Some(*id),
            _ => None,
        }
    }

    /// Right-click on an item of the open folder dropdown: the bar's menu for it, where
    /// the click was. The popup's coordinates are turned into the shell's (both are CSS
    /// px, the popup sits on screen next to the shell), and Esc returns to the dropdown.
    pub(super) fn show_folder_item_menu(&self, id: i64, x: f64, y: f64) {
        let Some(folder) = self.open_folder() else { return };
        if !self.folder_dropdown_has(id, true) || !x.is_finite() || !y.is_finite() {
            return;
        }
        let Some(main) = self.window() else { return };
        let popup_rect = match self.popup.try_borrow() {
            Ok(p) => p.as_ref().map(|p| p.window.rect()),
            Err(_) => None,
        };
        let Some(pr) = popup_rect else { return };
        let scale = main.scale();
        let (ox, oy) = main.client_to_screen(0, 0);
        let at = PopupRect {
            x: (f64::from(pr.x - ox) / scale + x.clamp(0.0, 4096.0)).max(0.0),
            y: (f64::from(pr.y - oy) / scale + y.clamp(0.0, 4096.0)).max(0.0),
            width: 240.0,
            height: 320.0,
        };
        self.menu_from_folder.set(Some(folder));
        self.show_bookmark_menu(Some(id), at);
    }

    /// Dragging inside the open folder dropdown: `id` (one of its items) to `index` of
    /// `parent`, the folder itself or one of its subfolders. The dropdown shows the result.
    pub(super) fn move_bookmark_in_folder(&self, id: i64, parent: i64, index: u32) {
        let Some(folder) = self.open_folder() else { return };
        let ok = self.folder_dropdown_has(id, true) && (parent == folder || self.folder_dropdown_has(parent, true));
        if !ok || parent == id {
            return;
        }
        let r = self.store.borrow_mut().move_bookmark(id, parent, index);
        self.bar_changed("move in folder", r);
        self.show_bookmark_folder(folder, None, None);
    }

    /// The active page dragged from the address bar onto the bar: the host takes the
    /// address and title from the tab, never from the shell.
    pub(super) fn bookmark_active_page(&self, parent: i64, index: u32) {
        let Some(tab) = self.active_tab() else { return };
        if !bookmarkable(&tab.url) {
            return;
        }
        let title = if tab.title.trim().is_empty() { tab.url.clone() } else { tab.title.clone() };
        let r = self.store.borrow_mut().add_bookmark_at_index(parent, &title, &tab.url, index);
        self.bar_changed("bookmark page", r);
    }

    /// An address dropped onto the bar (the address bar's selected text).
    pub(super) fn add_bookmark_at(&self, url: &str, parent: i64, index: u32) {
        let url = url.trim();
        if !bookmarkable(url) {
            return;
        }
        let r = self.store.borrow_mut().add_bookmark_at_index(parent, url, url, index);
        if r.is_ok() {
            let private = self.active_tab().is_some_and(|t| t.private);
            self.favicon_bookmark_added(url, private);
        }
        self.bar_changed("drop address", r);
    }

    /// Dragging on the bar: bookmark `id` into folder `parent` at `index`.
    pub(super) fn move_bookmark_from_bar(&self, id: i64, parent: i64, index: u32) {
        let r = self.store.borrow_mut().move_bookmark(id, parent, index);
        self.bar_changed("move", r);
    }

    fn bar_changed<T>(&self, what: &str, r: Result<T, lb_core::storage::DbError>) {
        if let Err(e) = r {
            warning!("bookmarks bar: {what}: {e}");
        }
        self.send_bookmarks_bar();
        self.send_nav();
    }

    /// A new tab behind the current one ("Open in new tab", "Open all").
    pub(super) fn open_in_background(&self, url: &str) {
        let Some(win) = self.cur_id() else { return };
        let opened = self.ws.borrow_mut().open(win, url, false, Position::End, false, now_ms());
        if let Some((_, ops)) = opened {
            self.apply(ops);
        }
    }
}
