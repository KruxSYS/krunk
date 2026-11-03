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

const SCHEMA_VERSION: i64 = 4;

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

/// v2: favicons are stored as PNG bytes (served from the internal origins)
/// instead of data: URLs.
const SCHEMA_V2: &str = "
DROP TABLE favicons;
CREATE TABLE favicons (
    host    TEXT PRIMARY KEY,
    png     BLOB NOT NULL,
    updated INTEGER NOT NULL
);
";

/// v3: per-site ad-blocking allowlist (it used to live in the settings JSON).
const SCHEMA_V3: &str = "
CREATE TABLE adblock_allowlist (
    host  TEXT PRIMARY KEY,
    added INTEGER NOT NULL
);
";

/// v4: origins where the user chose "Never save passwords" (no secrets).
const SCHEMA_V4: &str = "
CREATE TABLE vault_never (
    origin TEXT PRIMARY KEY,
    added  INTEGER NOT NULL
);
";

/// Largest favicon we keep.
pub const MAX_FAVICON_BYTES: usize = 48 * 1024;

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
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        }
        if version < 2 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V2)?;
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
        }
        if version < 3 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V3)?;
            // Move an allowlist from an older settings record into the table.
            let old: Option<String> =
                tx.query_row("SELECT value FROM kv WHERE key = 'settings'", [], |r| r.get(0)).optional()?;
            if let Some(json) = old {
                let hosts = serde_json::from_str::<serde_json::Value>(&json)
                    .ok()
                    .and_then(|v| v.get("adblockAllowlist").cloned())
                    .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
                    .unwrap_or_default();
                for h in hosts.iter().filter_map(|h| crate::blocker::normalize_host(h)) {
                    tx.execute("INSERT OR IGNORE INTO adblock_allowlist(host, added) VALUES (?1, 0)", [h])?;
                }
            }
            tx.pragma_update(None, "user_version", 3)?;
            tx.commit()?;
        }
        if version < 4 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V4)?;
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

    /// Stores a host's favicon (PNG bytes, as captured from the page).
    pub fn set_favicon(&self, host: &str, png: &[u8], now: i64) -> DbResult<()> {
        if png.is_empty() || png.len() > MAX_FAVICON_BYTES || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO favicons(host, png, updated) VALUES (?1, ?2, ?3)
             ON CONFLICT(host) DO UPDATE SET png = excluded.png, updated = excluded.updated",
            params![host, png, now],
        )?;
        Ok(())
    }

    pub fn favicon(&self, host: &str) -> DbResult<Option<Vec<u8>>> {
        self.conn.query_row("SELECT png FROM favicons WHERE host = ?1", [host], |r| r.get(0)).optional()
    }

    pub fn has_favicon(&self, host: &str) -> DbResult<bool> {
        Ok(self.conn.query_row("SELECT 1 FROM favicons WHERE host = ?1", [host], |_| Ok(())).optional()?.is_some())
    }

    /// Same-origin URL of a host's cached favicon, if there is one.
    pub fn favicon_url(&self, host: &str) -> DbResult<Option<String>> {
        Ok(self.has_favicon(host)?.then(|| crate::assets::favicon_url(host)))
    }

    /// All allowlisted hosts, sorted.
    pub fn allowlist(&self) -> DbResult<Vec<String>> {
        let mut stmt = self.conn.prepare_cached("SELECT host FROM adblock_allowlist ORDER BY host")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect()
    }

    /// Adds a host (normalized; invalid names are rejected). Returns the
    /// stored form.
    pub fn allowlist_add(&self, host: &str, now: i64) -> DbResult<String> {
        let h = crate::blocker::normalize_host(host)
            .ok_or_else(|| DbError::InvalidParameterName("Not a valid host name".into()))?;
        self.conn.execute("INSERT OR IGNORE INTO adblock_allowlist(host, added) VALUES (?1, ?2)", params![h, now])?;
        Ok(h)
    }

    pub fn allowlist_remove(&self, host: &str) -> DbResult<bool> {
        let Some(h) = crate::blocker::normalize_host(host) else { return Ok(false) };
        Ok(self.conn.execute("DELETE FROM adblock_allowlist WHERE host = ?1", [h])? > 0)
    }

    /// "Never save passwords" list: exact origins (`scheme://host[:port]`).
    pub fn never_save(&self, origin: &str) -> DbResult<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM vault_never WHERE origin = ?1", [origin], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub fn never_save_add(&self, origin: &str, now: i64) -> DbResult<()> {
        let o =
            crate::urlclean::origin_of(origin).ok_or_else(|| DbError::InvalidParameterName("Not an origin".into()))?;
        self.conn.execute("INSERT OR IGNORE INTO vault_never(origin, added) VALUES (?1, ?2)", params![o, now])?;
        Ok(())
    }

    pub fn never_save_remove(&self, origin: &str) -> DbResult<()> {
        self.conn.execute("DELETE FROM vault_never WHERE origin = ?1", [origin])?;
        Ok(())
    }

    pub fn never_save_list(&self) -> DbResult<Vec<String>> {
        let mut stmt = self.conn.prepare_cached("SELECT origin FROM vault_never ORDER BY origin")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect()
    }

    /// Persisted unlock rate limiter (failure count and time; no secrets).
    pub fn load_rate_limiter(&self) -> DbResult<crate::vault::RateLimiter> {
        Ok(self.kv_get("vault_rate")?.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default())
    }

    pub fn save_rate_limiter(&self, r: &crate::vault::RateLimiter) -> DbResult<()> {
        let json = serde_json::to_string(r).map_err(|e| DbError::ToSqlConversionFailure(Box::new(e)))?;
        self.kv_set("vault_rate", &json)
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
    fn allowlist_crud() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.allowlist_add("WWW.Example.com", 1).unwrap(), "www.example.com");
        assert_eq!(s.allowlist_add("*.news.org", 1).unwrap(), "news.org");
        s.allowlist_add("news.org", 2).unwrap();
        assert!(s.allowlist_add("bad host", 1).is_err());
        assert_eq!(s.allowlist().unwrap(), ["news.org", "www.example.com"]);
        assert!(s.allowlist_remove("NEWS.org").unwrap());
        assert!(!s.allowlist_remove("news.org").unwrap());
        assert_eq!(s.allowlist().unwrap(), ["www.example.com"]);
    }

    #[test]
    fn v3_migrates_settings_allowlist() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA_V1).unwrap();
        c.execute_batch(SCHEMA_V2).unwrap();
        c.pragma_update(None, "user_version", 2).unwrap();
        c.execute(
            "INSERT INTO kv(key, value) VALUES ('settings', ?1)",
            [r#"{"adblockAllowlist":["Shop.example","not valid"],"httpsOnly":true}"#],
        )
        .unwrap();
        let s = Store::init(c).unwrap();
        assert_eq!(s.allowlist().unwrap(), ["shop.example"]);
        assert!(s.load_settings().unwrap().https_only, "other settings kept");
    }

    #[test]
    fn never_save_list_and_rate_limiter() {
        let s = Store::open_in_memory().unwrap();
        s.never_save_add("https://bank.example/login?x=1", 1).unwrap();
        assert!(s.never_save("https://bank.example").unwrap());
        assert!(!s.never_save("https://bank.example:8443").unwrap());
        assert!(s.never_save_add("data:text/html,x", 1).is_err());
        assert_eq!(s.never_save_list().unwrap(), ["https://bank.example"]);
        s.never_save_remove("https://bank.example").unwrap();
        assert!(s.never_save_list().unwrap().is_empty());

        let mut r = s.load_rate_limiter().unwrap();
        assert_eq!(r.failures, 0);
        r.record_failure(5000);
        r.record_failure(6000);
        s.save_rate_limiter(&r).unwrap();
        let back = s.load_rate_limiter().unwrap();
        assert_eq!(back, r);
        assert_eq!(back.wait_ms(6000), 2000, "delay survives a restart");
    }

    #[test]
    fn like_escaping() {
        assert_eq!(like_pattern("50%_a\\b"), "%50\\%\\_a\\\\b%");
    }

    #[test]
    fn favicons_only_accept_png() {
        let s = Store::open_in_memory().unwrap();
        s.set_favicon("a.com", b"<svg onload=alert(1)>", 1).unwrap();
        assert_eq!(s.favicon("a.com").unwrap(), None);
        assert_eq!(s.favicon_url("a.com").unwrap(), None);
        let png = b"\x89PNG\r\n\x1a\nxyz";
        s.set_favicon("a.com", png, 1).unwrap();
        assert_eq!(s.favicon("a.com").unwrap().as_deref(), Some(&png[..]));
        assert_eq!(s.favicon_url("a.com").unwrap().as_deref(), Some("/favicon?host=a.com"));
        s.set_favicon("big.com", &[png.as_slice(), &[0u8; MAX_FAVICON_BYTES]].concat(), 1).unwrap();
        assert_eq!(s.favicon("big.com").unwrap(), None);
    }

    #[test]
    fn migrates_v1_database() {
        let dir = std::env::temp_dir().join(format!("lb-migrate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(SCHEMA_V1).unwrap();
            c.pragma_update(None, "user_version", 1).unwrap();
            c.execute(
                "INSERT INTO urls(url, title, visit_count, last_visit) VALUES ('https://a.example/', 'A', 1, 5)",
                [],
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.last_visit("https://a.example/").unwrap(), Some(5), "data kept");
        s.set_favicon("a.example", b"\x89PNG\r\n\x1a\n", 1).unwrap();
        let v: i64 = s.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        drop(s);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
