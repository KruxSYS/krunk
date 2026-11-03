//! SQLite database: schema, migrations and small key/value helpers.
//!
//! One database (`browser.db`) holds history, bookmarks, new-tab tiles,
//! settings and the saved session. Passwords are *never* stored here; they
//! live encrypted in the separate vault file (see `vault`).

use crate::settings::Settings;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub use rusqlite::Error as DbError;
pub type DbResult<T> = Result<T, DbError>;

/// Id of the "Bookmarks bar" root folder.
pub const BOOKMARKS_BAR: i64 = 1;
/// Id of the "Other bookmarks" root folder.
pub const OTHER_BOOKMARKS: i64 = 2;

const SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = "
CREATE TABLE urls (
    id          INTEGER PRIMARY KEY,
    url         TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL DEFAULT '',
    visit_count INTEGER NOT NULL DEFAULT 0,
    last_visit  INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE visits (
    id     INTEGER PRIMARY KEY,
    url_id INTEGER NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
    ts     INTEGER NOT NULL
);
CREATE INDEX visits_url_ts ON visits(url_id, ts);
CREATE INDEX visits_ts ON visits(ts);

CREATE TABLE bookmarks (
    id       INTEGER PRIMARY KEY,
    parent   INTEGER REFERENCES bookmarks(id) ON DELETE CASCADE,
    is_folder INTEGER NOT NULL,
    title    TEXT NOT NULL,
    url      TEXT,
    position INTEGER NOT NULL,
    added    INTEGER NOT NULL
);
CREATE INDEX bookmarks_parent ON bookmarks(parent, position);
CREATE INDEX bookmarks_url ON bookmarks(url);
INSERT INTO bookmarks(id, parent, is_folder, title, url, position, added)
    VALUES (1, NULL, 1, 'Bookmarks bar', NULL, 0, 0),
           (2, NULL, 1, 'Other bookmarks', NULL, 1, 0);

-- New-tab tiles the user pinned (state 1, ordered by position) or removed (state 2).
CREATE TABLE tiles (
    url      TEXT PRIMARY KEY,
    title    TEXT NOT NULL DEFAULT '',
    state    INTEGER NOT NULL,
    position INTEGER NOT NULL DEFAULT 0
);

-- Favicons as data: URLs, keyed by host.
CREATE TABLE favicons (
    host TEXT PRIMARY KEY,
    data TEXT NOT NULL
);

CREATE TABLE kv (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

/// Handle to the browser database.
pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> DbResult<Store> {
        Store::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> DbResult<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> DbResult<Store> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Deleted history should really be gone from the file.
        conn.pragma_update(None, "secure_delete", "ON")?;
        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> DbResult<()> {
        let version: i64 = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V1)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn kv_get(&self, key: &str) -> DbResult<Option<String>> {
        self.conn.query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0)).optional()
    }

    pub fn kv_set(&self, key: &str, value: &str) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO kv(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn kv_delete(&self, key: &str) -> DbResult<()> {
        self.conn.execute("DELETE FROM kv WHERE key = ?1", [key])?;
        Ok(())
    }

    /// Loads settings; a missing or corrupt record yields valid defaults.
    pub fn load_settings(&self) -> DbResult<Settings> {
        Ok(self.kv_get("settings")?.map(|j| Settings::from_json_lenient(&j)).unwrap_or_default())
    }

    /// Saves settings. Callers must have validated them.
    pub fn save_settings(&self, s: &Settings) -> DbResult<()> {
        let json = serde_json::to_string(s).map_err(|e| DbError::ToSqlConversionFailure(Box::new(e)))?;
        self.kv_set("settings", &json)
    }

    /// Stores the favicon (a `data:image/...` URL) for a host.
    pub fn set_favicon(&self, host: &str, data_url: &str) -> DbResult<()> {
        if !data_url.starts_with("data:image/") || data_url.len() > 64 * 1024 {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO favicons(host, data) VALUES (?1, ?2) ON CONFLICT(host) DO UPDATE SET data = excluded.data",
            params![host, data_url],
        )?;
        Ok(())
    }

    pub fn favicon(&self, host: &str) -> DbResult<Option<String>> {
        self.conn.query_row("SELECT data FROM favicons WHERE host = ?1", [host], |r| r.get(0)).optional()
    }

    /// Reclaims space after large deletions.
    pub fn vacuum(&self) -> DbResult<()> {
        self.conn.execute_batch("VACUUM")
    }
}

/// Escapes `%`, `_` and `\` for a `LIKE ... ESCAPE '\'` pattern.
pub(crate) fn like_pattern(text: &str) -> String {
    let mut p = String::with_capacity(text.len() + 2);
    p.push('%');
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            p.push('\\');
        }
        p.push(c);
    }
    p.push('%');
    p
}

/// Current time in Unix milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_migrates_idempotently() {
        let dir = std::env::temp_dir().join(format!("lb-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("browser.db");
        {
            let s = Store::open(&path).unwrap();
            s.kv_set("a", "1").unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.kv_get("a").unwrap().as_deref(), Some("1"));
        drop(s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_persist() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.load_settings().unwrap(), Settings::default());
        let custom = Settings { https_only: true, tab_sleep_minutes: 12, ..Settings::default() };
        s.save_settings(&custom).unwrap();
        assert_eq!(s.load_settings().unwrap(), custom);
        s.kv_set("settings", "{broken").unwrap();
        assert_eq!(s.load_settings().unwrap(), Settings::default());
    }

    #[test]
    fn like_escaping() {
        assert_eq!(like_pattern("50%_a\\b"), "%50\\%\\_a\\\\b%");
    }

    #[test]
    fn favicons_only_accept_image_data_urls() {
        let s = Store::open_in_memory().unwrap();
        s.set_favicon("a.com", "javascript:alert(1)").unwrap();
        assert_eq!(s.favicon("a.com").unwrap(), None);
        s.set_favicon("a.com", "data:image/png;base64,AAAA").unwrap();
        assert_eq!(s.favicon("a.com").unwrap().as_deref(), Some("data:image/png;base64,AAAA"));
    }
}
