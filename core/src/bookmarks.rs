//! Bookmarks: a folder tree with two fixed roots (bar and "other").

use crate::storage::{like_pattern, now_ms, DbError, DbResult, Store, BOOKMARKS_BAR, OTHER_BOOKMARKS};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: i64,
    pub parent: Option<i64>,
    pub is_folder: bool,
    pub title: String,
    pub url: Option<String>,
    pub position: i64,
    /// Unix ms.
    pub added: i64,
}

/// A bookmark with its children, for the manager page and export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkNode {
    #[serde(flatten)]
    pub item: Bookmark,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<BookmarkNode>,
}

pub const MAX_TITLE_CHARS: usize = 1024;

/// The two fixed folders (bookmarks bar, other bookmarks) cannot be edited, moved or deleted.
pub fn is_root(id: i64) -> bool {
    id == BOOKMARKS_BAR || id == OTHER_BOOKMARKS
}

fn invalid(msg: &str) -> DbError {
    DbError::InvalidParameterName(msg.to_string())
}

/// Only web and file URLs can be bookmarked (no `javascript:` bookmarklets).
pub fn bookmarkable(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https" | "file")) && url.len() <= 8192
}

fn clean_title(t: &str) -> String {
    t.chars().filter(|c| !c.is_control()).take(MAX_TITLE_CHARS).collect::<String>().trim().to_string()
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark {
        id: r.get(0)?,
        parent: r.get(1)?,
        is_folder: r.get(2)?,
        title: r.get(3)?,
        url: r.get(4)?,
        position: r.get(5)?,
        added: r.get(6)?,
    })
}

const COLS: &str = "id, parent, is_folder, title, url, position, added";

/// A new bookmark's sync guid: 16 random bytes (PROTOCOL §9.5).
fn new_guid() -> DbResult<[u8; 16]> {
    let mut g = [0u8; 16];
    getrandom::fill(&mut g)
        .map_err(|e| DbError::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string()))))?;
    Ok(g)
}

impl Store {
    pub fn bookmark(&self, id: i64) -> DbResult<Option<Bookmark>> {
        self.conn.query_row(&format!("SELECT {COLS} FROM bookmarks WHERE id = ?1"), [id], row).optional()
    }

    fn require_folder(&self, id: i64) -> DbResult<()> {
        match self.bookmark(id)? {
            Some(b) if b.is_folder => Ok(()),
            _ => Err(invalid("parent is not a folder")),
        }
    }

    fn next_position(&self, parent: i64) -> DbResult<i64> {
        self.conn.query_row("SELECT coalesce(max(position), -1) + 1 FROM bookmarks WHERE parent = ?1", [parent], |r| {
            r.get(0)
        })
    }

    fn insert(&self, parent: i64, is_folder: bool, title: &str, url: Option<&str>, added: i64) -> DbResult<i64> {
        self.require_folder(parent)?;
        let pos = self.next_position(parent)?;
        self.conn.execute(
            "INSERT INTO bookmarks(parent, is_folder, title, url, position, added, guid, modified)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6)",
            params![parent, is_folder, clean_title(title), url, pos, added, new_guid()?.to_vec()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Adds a bookmark at the end of `parent`.
    pub fn add_bookmark(&self, parent: i64, title: &str, url: &str) -> DbResult<i64> {
        self.add_bookmark_at(parent, title, url, now_ms())
    }

    /// A bookmark dropped at `index` of `parent` (the bookmarks bar or a folder): added
    /// at the end, then moved into place.
    pub fn add_bookmark_at_index(&mut self, parent: i64, title: &str, url: &str, index: u32) -> DbResult<i64> {
        let id = self.add_bookmark(parent, title, url)?;
        self.move_bookmark(id, parent, index)?;
        Ok(id)
    }

    pub(crate) fn add_bookmark_at(&self, parent: i64, title: &str, url: &str, added: i64) -> DbResult<i64> {
        if !bookmarkable(url) {
            return Err(invalid("url cannot be bookmarked"));
        }
        let title = if title.trim().is_empty() { url } else { title };
        self.insert(parent, false, title, Some(url), added)
    }

    pub fn add_folder(&self, parent: i64, title: &str) -> DbResult<i64> {
        self.add_folder_at(parent, title, now_ms())
    }

    pub(crate) fn add_folder_at(&self, parent: i64, title: &str, added: i64) -> DbResult<i64> {
        let title = if title.trim().is_empty() { "New folder" } else { title };
        self.insert(parent, true, title, None, added)
    }

    /// Renames a bookmark or folder; `url` changes a bookmark's address.
    pub fn edit_bookmark(&self, id: i64, title: &str, url: Option<&str>) -> DbResult<()> {
        if is_root(id) {
            return Err(invalid("cannot edit root folder"));
        }
        let b = self.bookmark(id)?.ok_or(DbError::QueryReturnedNoRows)?;
        let url = match (b.is_folder, url) {
            (true, Some(_)) => return Err(invalid("folders have no url")),
            (false, Some(u)) if !bookmarkable(u) => return Err(invalid("url cannot be bookmarked")),
            (false, Some(u)) => Some(u.to_string()),
            (_, None) => b.url,
        };
        self.conn.execute(
            "UPDATE bookmarks SET title = ?2, url = ?3, modified = ?4 WHERE id = ?1",
            params![id, clean_title(title), url, now_ms()],
        )?;
        Ok(())
    }

    /// Moves `id` into `parent` at `index` (clamped). Folders cannot be moved
    /// into themselves or their descendants.
    pub fn move_bookmark(&mut self, id: i64, parent: i64, index: u32) -> DbResult<()> {
        if is_root(id) {
            return Err(invalid("cannot move root folder"));
        }
        self.require_folder(parent)?;
        let b = self.bookmark(id)?.ok_or(DbError::QueryReturnedNoRows)?;
        // Walk up from the target parent; hitting `id` means a cycle.
        let mut cursor = Some(parent);
        while let Some(c) = cursor {
            if c == id {
                return Err(invalid("cannot move a folder into itself"));
            }
            cursor = self.bookmark(c)?.and_then(|x| x.parent);
        }
        let tx = self.conn.transaction()?;
        let siblings: Vec<i64> = {
            let mut stmt = tx.prepare("SELECT id FROM bookmarks WHERE parent = ?1 AND id <> ?2 ORDER BY position")?;
            let rows = stmt.query_map(params![parent, id], |r| r.get(0))?;
            rows.collect::<DbResult<_>>()?
        };
        let mut order = siblings;
        order.insert((index as usize).min(order.len()), id);
        tx.execute("UPDATE bookmarks SET parent = ?2, modified = ?3 WHERE id = ?1", params![id, parent, now_ms()])?;
        for (pos, bid) in order.iter().enumerate() {
            // Only rows whose place changes are written (each write is a change for sync).
            tx.execute(
                "UPDATE bookmarks SET position = ?2 WHERE id = ?1 AND position <> ?2",
                params![bid, pos as i64],
            )?;
        }
        if b.parent != Some(parent) {
            if let Some(old) = b.parent {
                renumber(&tx, old)?;
            }
        }
        tx.commit()
    }

    /// Deletes a bookmark or a folder with everything in it.
    pub fn delete_bookmark(&mut self, id: i64) -> DbResult<()> {
        if is_root(id) {
            return Err(invalid("cannot delete root folder"));
        }
        let b = self.bookmark(id)?.ok_or(DbError::QueryReturnedNoRows)?;
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM bookmarks WHERE id = ?1", [id])?;
        if let Some(p) = b.parent {
            renumber(&tx, p)?;
        }
        tx.commit()
    }

    pub fn children(&self, parent: i64) -> DbResult<Vec<Bookmark>> {
        let mut stmt =
            self.conn.prepare_cached(&format!("SELECT {COLS} FROM bookmarks WHERE parent = ?1 ORDER BY position"))?;
        let rows = stmt.query_map([parent], row)?;
        rows.collect()
    }

    /// The whole tree: `[bookmarks bar, other bookmarks]`.
    pub fn bookmark_tree(&self) -> DbResult<Vec<BookmarkNode>> {
        [BOOKMARKS_BAR, OTHER_BOOKMARKS]
            .iter()
            .map(|&id| {
                let item = self.bookmark(id)?.ok_or(DbError::QueryReturnedNoRows)?;
                self.subtree(item, 0)
            })
            .collect()
    }

    fn subtree(&self, item: Bookmark, depth: u32) -> DbResult<BookmarkNode> {
        let children = if item.is_folder && depth < 64 {
            self.children(item.id)?.into_iter().map(|c| self.subtree(c, depth + 1)).collect::<DbResult<_>>()?
        } else {
            Vec::new()
        };
        Ok(BookmarkNode { item, children })
    }

    /// Bookmarks (not folders) whose title or URL contains `text`.
    pub fn search_bookmarks(&self, text: &str) -> DbResult<Vec<Bookmark>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {COLS} FROM bookmarks WHERE is_folder = 0
             AND (title LIKE ?1 ESCAPE '\\' OR url LIKE ?1 ESCAPE '\\') ORDER BY title LIMIT 500"
        ))?;
        let rows = stmt.query_map([like_pattern(text.trim())], row)?;
        rows.collect()
    }

    /// Ids of bookmarks pointing exactly at `url` (for the star button).
    pub fn bookmarks_for_url(&self, url: &str) -> DbResult<Vec<i64>> {
        let mut stmt = self.conn.prepare_cached("SELECT id FROM bookmarks WHERE is_folder = 0 AND url = ?1")?;
        let rows = stmt.query_map([url], |r| r.get(0))?;
        rows.collect()
    }

    /// Children of `folder` for the bookmarks bar or a folder dropdown.
    pub fn bar_items(&self, folder: i64) -> DbResult<Vec<crate::protocol::BarItem>> {
        self.children(folder)?
            .into_iter()
            .map(|b| {
                let favicon = match b.url.as_deref().and_then(crate::urlclean::host_of) {
                    Some(h) => self.favicon_url(&h)?,
                    None => None,
                };
                Ok(crate::protocol::BarItem { id: b.id, title: b.title, url: b.url, is_folder: b.is_folder, favicon })
            })
            .collect()
    }

    pub fn is_bookmarked(&self, url: &str) -> DbResult<bool> {
        Ok(!self.bookmarks_for_url(url)?.is_empty())
    }

    /// Star button: removes all bookmarks for `url` if any exist, otherwise
    /// adds one to the bookmarks bar. Returns the new bookmarked state.
    pub fn toggle_bookmark(&mut self, url: &str, title: &str) -> DbResult<bool> {
        let ids = self.bookmarks_for_url(url)?;
        if ids.is_empty() {
            self.add_bookmark(BOOKMARKS_BAR, title, url)?;
            Ok(true)
        } else {
            for id in ids {
                self.delete_bookmark(id)?;
            }
            Ok(false)
        }
    }
}

fn renumber(tx: &rusqlite::Transaction<'_>, parent: i64) -> DbResult<()> {
    let ids: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT id FROM bookmarks WHERE parent = ?1 ORDER BY position, id")?;
        let rows = stmt.query_map([parent], |r| r.get(0))?;
        rows.collect::<DbResult<_>>()?
    };
    for (pos, id) in ids.iter().enumerate() {
        tx.execute("UPDATE bookmarks SET position = ?2 WHERE id = ?1 AND position <> ?2", params![id, pos as i64])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(s: &Store, parent: i64) -> Vec<String> {
        s.children(parent).unwrap().into_iter().map(|b| b.title).collect()
    }

    #[test]
    fn add_toggle_and_lookup() {
        let mut s = Store::open_in_memory().unwrap();
        assert!(s.toggle_bookmark("https://rust-lang.org/", "Rust").unwrap());
        assert!(s.is_bookmarked("https://rust-lang.org/").unwrap());
        assert_eq!(titles(&s, BOOKMARKS_BAR), ["Rust"]);
        assert!(!s.toggle_bookmark("https://rust-lang.org/", "Rust").unwrap());
        assert!(!s.is_bookmarked("https://rust-lang.org/").unwrap());
        assert!(s.add_bookmark(BOOKMARKS_BAR, "x", "javascript:alert(1)").is_err());
    }

    #[test]
    fn folders_rename_move_delete() {
        let mut s = Store::open_in_memory().unwrap();
        let dev = s.add_folder(BOOKMARKS_BAR, "Dev").unwrap();
        let a = s.add_bookmark(dev, "A", "https://a.example/").unwrap();
        let b = s.add_bookmark(dev, "B", "https://b.example/").unwrap();
        let c = s.add_bookmark(BOOKMARKS_BAR, "C", "https://c.example/").unwrap();
        assert_eq!(titles(&s, BOOKMARKS_BAR), ["Dev", "C"]);

        s.edit_bookmark(a, "Alpha", Some("https://alpha.example/")).unwrap();
        assert_eq!(s.bookmark(a).unwrap().unwrap().url.as_deref(), Some("https://alpha.example/"));
        assert!(s.edit_bookmark(dev, "Dev", Some("https://x/")).is_err());

        // Reorder within a folder, then move across folders.
        s.move_bookmark(b, dev, 0).unwrap();
        assert_eq!(titles(&s, dev), ["B", "Alpha"]);
        s.move_bookmark(c, dev, 1).unwrap();
        assert_eq!(titles(&s, dev), ["B", "C", "Alpha"]);
        assert_eq!(titles(&s, BOOKMARKS_BAR), ["Dev"]);

        // Cycles and roots are rejected.
        let sub = s.add_folder(dev, "Sub").unwrap();
        assert!(s.move_bookmark(dev, sub, 0).is_err());
        assert!(s.move_bookmark(dev, dev, 0).is_err());
        assert!(s.move_bookmark(BOOKMARKS_BAR, OTHER_BOOKMARKS, 0).is_err());
        assert!(s.delete_bookmark(OTHER_BOOKMARKS).is_err());
        assert!(s.move_bookmark(a, b, 0).is_err(), "bookmark is not a folder");

        // Deleting a folder deletes its contents.
        s.delete_bookmark(dev).unwrap();
        assert!(s.bookmark(a).unwrap().is_none());
        assert!(s.bookmark(sub).unwrap().is_none());
        assert!(titles(&s, BOOKMARKS_BAR).is_empty());
    }

    #[test]
    fn search_and_tree() {
        let s = Store::open_in_memory().unwrap();
        s.add_bookmark(BOOKMARKS_BAR, "Rust docs", "https://doc.rust-lang.org/").unwrap();
        let f = s.add_folder(OTHER_BOOKMARKS, "News").unwrap();
        s.add_bookmark(f, "Example News", "https://news.example/").unwrap();
        assert_eq!(s.search_bookmarks("rust").unwrap().len(), 1);
        assert_eq!(s.search_bookmarks("NEWS").unwrap().len(), 1, "folders are not search results");
        let tree = s.bookmark_tree().unwrap();
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[1].children[0].children[0].item.title, "Example News");
    }

    #[test]
    fn bar_items_include_folders_and_favicons() {
        let s = Store::open_in_memory().unwrap();
        s.add_bookmark(BOOKMARKS_BAR, "Rust", "https://www.rust-lang.org/").unwrap();
        let f = s.add_folder(BOOKMARKS_BAR, "News").unwrap();
        s.add_bookmark(f, "BBC", "https://www.bbc.co.uk/news").unwrap();
        s.set_favicon("www.rust-lang.org", b"\x89PNG\r\n\x1a\n", 1).unwrap();
        let bar = s.bar_items(BOOKMARKS_BAR).unwrap();
        assert_eq!(bar.len(), 2);
        assert_eq!(bar[0].favicon.as_deref(), Some("/favicon?host=www.rust-lang.org"));
        assert!(bar[1].is_folder && bar[1].url.is_none());
        assert_eq!(s.bar_items(f).unwrap()[0].title, "BBC");
    }

    #[test]
    fn add_at_index_places_the_bookmark() {
        let mut s = Store::open_in_memory().unwrap();
        let a = s.add_bookmark(BOOKMARKS_BAR, "A", "https://a.example/").unwrap();
        let c = s.add_bookmark(BOOKMARKS_BAR, "C", "https://c.example/").unwrap();
        let b = s.add_bookmark_at_index(BOOKMARKS_BAR, "B", "https://b.example/", 1).unwrap();
        let order: Vec<i64> = s.children(BOOKMARKS_BAR).unwrap().iter().map(|x| x.id).collect();
        assert_eq!(order, vec![a, b, c]);
        // Past the end lands at the end; into a folder works the same.
        let d = s.add_bookmark_at_index(BOOKMARKS_BAR, "D", "https://d.example/", 99).unwrap();
        let f = s.add_folder(BOOKMARKS_BAR, "F").unwrap();
        let e = s.add_bookmark_at_index(f, "E", "https://e.example/", 0).unwrap();
        let order: Vec<i64> = s.children(BOOKMARKS_BAR).unwrap().iter().map(|x| x.id).collect();
        assert_eq!(order, vec![a, b, c, d, f]);
        assert_eq!(s.children(f).unwrap()[0].id, e);
        assert!(s.add_bookmark_at_index(BOOKMARKS_BAR, "X", "krunk://settings", 0).is_err());
    }

    #[test]
    fn titles_are_sanitized() {
        let s = Store::open_in_memory().unwrap();
        let id = s.add_bookmark(BOOKMARKS_BAR, " a\u{0}b\n ", "https://x.example/").unwrap();
        assert_eq!(s.bookmark(id).unwrap().unwrap().title, "ab");
        let id = s.add_bookmark(BOOKMARKS_BAR, "", "https://y.example/").unwrap();
        assert_eq!(s.bookmark(id).unwrap().unwrap().title, "https://y.example/");
    }
}
