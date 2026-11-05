//! The bookmarks bar (0.4.0): where it shows (always, only on the new-tab page, never), its
//! context menu (on a bookmark, a folder or the bar itself), the small editor for names and
//! addresses, and moves from dragging items on the bar or onto its folders.

use super::Browser;
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
        self.show_popup(PopupContent::BookmarkMenu { id, is_folder, title, url, children, mode, can_add }, Some(at));
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
                self.show_popup(content, Some(layout::bookmark_edit_rect(at.x, at.y, is_folder)));
                return;
            }
            _ => self.hide_popup(),
        }
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
    }

    /// The editor's Save: rename or edit bookmark `id`, or a new folder in `parent`.
    pub(super) fn save_bookmark(&self, id: Option<i64>, parent: i64, title: &str, url: Option<&str>) {
        self.hide_popup();
        let r = match id {
            Some(id) => self.store.borrow().edit_bookmark(id, title, url),
            None => self.store.borrow().add_folder(parent, title).map(|_| ()),
        };
        self.bar_changed("edit", r);
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
    fn open_in_background(&self, url: &str) {
        let Some(win) = self.cur_id() else { return };
        let opened = self.ws.borrow_mut().open(win, url, false, Position::End, false, now_ms());
        if let Some((_, ops)) = opened {
            self.apply(ops);
        }
    }
}
