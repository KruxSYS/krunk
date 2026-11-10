//! Storage for 0.12.0: userscripts, their GM values, macros and the AI providers' protected keys.
//!
//! All local: no sync triggers, not part of settings export, import or reset. The AI keys are
//! stored as the DPAPI blobs the host hands in; this module never sees a plain key.

use crate::storage::{DbResult, Store};
use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;

pub(crate) const SCHEMA_V7: &str = "
CREATE TABLE userscripts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    namespace TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_by_ai INTEGER NOT NULL DEFAULT 0,
    created INTEGER NOT NULL,
    updated INTEGER NOT NULL,
    last_run INTEGER,
    position INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE userscript_values (
    script_id TEXT NOT NULL REFERENCES userscripts(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (script_id, key)
);
CREATE TABLE macros (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    source TEXT NOT NULL,
    hotkey TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_by_ai INTEGER NOT NULL DEFAULT 0,
    created INTEGER NOT NULL,
    updated INTEGER NOT NULL,
    last_run INTEGER,
    position INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX macros_hotkey ON macros(hotkey) WHERE hotkey IS NOT NULL;
CREATE TABLE ai_keys (
    provider_id TEXT PRIMARY KEY,
    blob BLOB NOT NULL,
    updated INTEGER NOT NULL
);
";

/// A stored userscript. `name`, `namespace` and `version` are copies of its metadata (for the
/// list); the source is the truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserscriptRow {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub version: String,
    pub source: String,
    pub enabled: bool,
    pub created_by_ai: bool,
    pub created: i64,
    pub updated: i64,
    pub last_run: Option<i64>,
    pub position: i64,
}

/// A stored macro. `hotkey` is the canonical chord text ("Ctrl+Alt+K").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroRow {
    pub id: String,
    pub name: String,
    pub source: String,
    pub hotkey: Option<String>,
    pub enabled: bool,
    pub created_by_ai: bool,
    pub created: i64,
    pub updated: i64,
    pub last_run: Option<i64>,
    pub position: i64,
}

const US_COLS: &str =
    "id, name, namespace, version, source, enabled, created_by_ai, created, updated, last_run, position";
const MACRO_COLS: &str = "id, name, source, hotkey, enabled, created_by_ai, created, updated, last_run, position";

fn us_row(r: &Row) -> rusqlite::Result<UserscriptRow> {
    Ok(UserscriptRow {
        id: r.get(0)?,
        name: r.get(1)?,
        namespace: r.get(2)?,
        version: r.get(3)?,
        source: r.get(4)?,
        enabled: r.get(5)?,
        created_by_ai: r.get(6)?,
        created: r.get(7)?,
        updated: r.get(8)?,
        last_run: r.get(9)?,
        position: r.get(10)?,
    })
}

fn macro_row(r: &Row) -> rusqlite::Result<MacroRow> {
    Ok(MacroRow {
        id: r.get(0)?,
        name: r.get(1)?,
        source: r.get(2)?,
        hotkey: r.get(3)?,
        enabled: r.get(4)?,
        created_by_ai: r.get(5)?,
        created: r.get(6)?,
        updated: r.get(7)?,
        last_run: r.get(8)?,
        position: r.get(9)?,
    })
}

/// A new random id: 32 lower-case hex digits.
pub fn new_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("system random source");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Is `id` one of ours (1–64 of `[0-9a-z-]`)? Ids from pages and the agent are checked first.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'-')
}

impl Store {
    // ---- userscripts ----

    pub fn userscripts(&self) -> DbResult<Vec<UserscriptRow>> {
        let mut st = self.conn.prepare(&format!("SELECT {US_COLS} FROM userscripts ORDER BY position, created"))?;
        let rows = st.query_map([], us_row)?;
        rows.collect()
    }

    pub fn userscript(&self, id: &str) -> DbResult<Option<UserscriptRow>> {
        self.conn.query_row(&format!("SELECT {US_COLS} FROM userscripts WHERE id = ?1"), [id], us_row).optional()
    }

    /// Inserts or replaces a script (keeps `created`, `last_run`, `position` and its values when
    /// it exists; a new one goes to the end).
    pub fn save_userscript(&self, s: &UserscriptRow) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO userscripts(id, name, namespace, version, source, enabled, created_by_ai, created, updated, last_run, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, (SELECT coalesce(max(position), 0) + 1 FROM userscripts))
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, namespace = excluded.namespace,
                version = excluded.version, source = excluded.source, enabled = excluded.enabled,
                created_by_ai = excluded.created_by_ai, updated = excluded.updated",
            params![s.id, s.name, s.namespace, s.version, s.source, s.enabled, s.created_by_ai, s.created, s.updated],
        )?;
        Ok(())
    }

    /// Returns false when there is no such script.
    pub fn set_userscript_enabled(&self, id: &str, enabled: bool, now: i64) -> DbResult<bool> {
        Ok(self
            .conn
            .execute("UPDATE userscripts SET enabled = ?2, updated = ?3 WHERE id = ?1", params![id, enabled, now])?
            > 0)
    }

    pub fn delete_userscript(&self, id: &str) -> DbResult<bool> {
        Ok(self.conn.execute("DELETE FROM userscripts WHERE id = ?1", [id])? > 0)
    }

    pub fn touch_userscript_run(&self, id: &str, now: i64) -> DbResult<()> {
        self.conn.execute("UPDATE userscripts SET last_run = ?2 WHERE id = ?1", params![id, now])?;
        Ok(())
    }

    /// A script's GM values (key → JSON text), by key.
    pub fn gm_values(&self, script: &str) -> DbResult<Vec<(String, String)>> {
        let mut st = self.conn.prepare("SELECT key, value FROM userscript_values WHERE script_id = ?1 ORDER BY key")?;
        let rows = st.query_map([script], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// Bytes of keys and values a script stores.
    pub fn gm_size(&self, script: &str) -> DbResult<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT coalesce(sum(length(CAST(key AS BLOB)) + length(CAST(value AS BLOB))), 0) FROM userscript_values WHERE script_id = ?1",
            [script],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Sets a value; refused (Ok(false)) when the script would store more than `max_bytes`, or
    /// doesn't exist.
    pub fn gm_set(&self, script: &str, key: &str, value: &str, max_bytes: usize) -> DbResult<bool> {
        let tx = self.conn.unchecked_transaction()?;
        let exists: bool = tx.query_row("SELECT count(*) FROM userscripts WHERE id = ?1", [script], |r| r.get(0))?;
        if !exists {
            return Ok(false);
        }
        let old: i64 = tx
            .query_row(
                "SELECT length(CAST(key AS BLOB)) + length(CAST(value AS BLOB)) FROM userscript_values WHERE script_id = ?1 AND key = ?2",
                params![script, key],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let total: i64 = tx.query_row(
            "SELECT coalesce(sum(length(CAST(key AS BLOB)) + length(CAST(value AS BLOB))), 0) FROM userscript_values WHERE script_id = ?1",
            [script],
            |r| r.get(0),
        )?;
        if (total - old) as usize + key.len() + value.len() > max_bytes {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO userscript_values(script_id, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(script_id, key) DO UPDATE SET value = excluded.value",
            params![script, key, value],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn gm_delete(&self, script: &str, key: &str) -> DbResult<()> {
        self.conn.execute("DELETE FROM userscript_values WHERE script_id = ?1 AND key = ?2", params![script, key])?;
        Ok(())
    }

    // ---- macros ----

    pub fn macros(&self) -> DbResult<Vec<MacroRow>> {
        let mut st = self.conn.prepare(&format!("SELECT {MACRO_COLS} FROM macros ORDER BY position, created"))?;
        let rows = st.query_map([], macro_row)?;
        rows.collect()
    }

    pub fn macro_by_id(&self, id: &str) -> DbResult<Option<MacroRow>> {
        self.conn.query_row(&format!("SELECT {MACRO_COLS} FROM macros WHERE id = ?1"), [id], macro_row).optional()
    }

    /// Inserts or replaces a macro (keeps `created`, `last_run` and `position`). Fails with a
    /// constraint error when another macro has the same hotkey (callers check first).
    pub fn save_macro(&self, m: &MacroRow) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO macros(id, name, source, hotkey, enabled, created_by_ai, created, updated, last_run, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, (SELECT coalesce(max(position), 0) + 1 FROM macros))
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, source = excluded.source, hotkey = excluded.hotkey,
                enabled = excluded.enabled, created_by_ai = excluded.created_by_ai, updated = excluded.updated",
            params![m.id, m.name, m.source, m.hotkey, m.enabled, m.created_by_ai, m.created, m.updated],
        )?;
        Ok(())
    }

    pub fn set_macro_enabled(&self, id: &str, enabled: bool, now: i64) -> DbResult<bool> {
        Ok(self
            .conn
            .execute("UPDATE macros SET enabled = ?2, updated = ?3 WHERE id = ?1", params![id, enabled, now])?
            > 0)
    }

    pub fn delete_macro(&self, id: &str) -> DbResult<bool> {
        Ok(self.conn.execute("DELETE FROM macros WHERE id = ?1", [id])? > 0)
    }

    pub fn touch_macro_run(&self, id: &str, now: i64) -> DbResult<()> {
        self.conn.execute("UPDATE macros SET last_run = ?2 WHERE id = ?1", params![id, now])?;
        Ok(())
    }

    // ---- AI provider keys (DPAPI blobs from the host) ----

    pub fn ai_key_blob(&self, provider: &str) -> DbResult<Option<Vec<u8>>> {
        self.conn.query_row("SELECT blob FROM ai_keys WHERE provider_id = ?1", [provider], |r| r.get(0)).optional()
    }

    pub fn set_ai_key_blob(&self, provider: &str, blob: &[u8], now: i64) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO ai_keys(provider_id, blob, updated) VALUES (?1, ?2, ?3)
             ON CONFLICT(provider_id) DO UPDATE SET blob = excluded.blob, updated = excluded.updated",
            params![provider, blob, now],
        )?;
        Ok(())
    }

    pub fn delete_ai_key(&self, provider: &str) -> DbResult<()> {
        self.conn.execute("DELETE FROM ai_keys WHERE provider_id = ?1", [provider])?;
        Ok(())
    }

    /// Providers that have a stored key.
    pub fn ai_key_providers(&self) -> DbResult<Vec<String>> {
        let mut st = self.conn.prepare("SELECT provider_id FROM ai_keys ORDER BY provider_id")?;
        let rows = st.query_map([], |r| r.get(0))?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(id: &str, name: &str) -> UserscriptRow {
        UserscriptRow {
            id: id.into(),
            name: name.into(),
            namespace: "ns".into(),
            version: "1.0".into(),
            source: format!("// ==UserScript==\n// @name {name}\n// ==/UserScript==\n"),
            enabled: true,
            created_by_ai: false,
            created: 10,
            updated: 10,
            last_run: None,
            position: 0,
        }
    }

    fn mac(id: &str, hotkey: Option<&str>) -> MacroRow {
        MacroRow {
            id: id.into(),
            name: format!("m {id}"),
            source: "await key.press('a')".into(),
            hotkey: hotkey.map(str::to_string),
            enabled: true,
            created_by_ai: true,
            created: 5,
            updated: 5,
            last_run: None,
            position: 0,
        }
    }

    #[test]
    fn userscripts_crud_and_values_cascade() {
        let s = Store::open_in_memory().unwrap();
        s.save_userscript(&script("a", "First")).unwrap();
        s.save_userscript(&script("b", "Second")).unwrap();
        assert_eq!(s.userscripts().unwrap().iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["First", "Second"]);
        // Update keeps created and position; last run is kept too.
        s.touch_userscript_run("a", 99).unwrap();
        let mut a = script("a", "First v2");
        a.created = 50;
        a.updated = 60;
        s.save_userscript(&a).unwrap();
        let a = s.userscript("a").unwrap().unwrap();
        assert_eq!((a.name.as_str(), a.created, a.updated, a.last_run), ("First v2", 10, 60, Some(99)));
        assert!(s.set_userscript_enabled("a", false, 70).unwrap());
        assert!(!s.userscript("a").unwrap().unwrap().enabled);
        assert!(!s.set_userscript_enabled("zz", false, 70).unwrap());
        // Values, with the size cap.
        assert!(s.gm_set("a", "k", "\"v\"", 1000).unwrap());
        assert!(s.gm_set("a", "k", "\"w\"", 1000).unwrap());
        assert!(!s.gm_set("a", "big", &"x".repeat(2000), 1000).unwrap());
        assert!(!s.gm_set("nope", "k", "1", 1000).unwrap());
        assert_eq!(s.gm_values("a").unwrap(), vec![("k".to_string(), "\"w\"".to_string())]);
        assert_eq!(s.gm_size("a").unwrap(), 4);
        s.gm_delete("a", "k").unwrap();
        assert!(s.gm_values("a").unwrap().is_empty());
        s.gm_set("a", "k2", "1", 1000).unwrap();
        assert!(s.delete_userscript("a").unwrap());
        let n: i64 = s.conn.query_row("SELECT count(*) FROM userscript_values", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "values go with their script");
    }

    #[test]
    fn macros_crud_and_unique_hotkeys() {
        let s = Store::open_in_memory().unwrap();
        s.save_macro(&mac("m1", Some("Ctrl+Alt+K"))).unwrap();
        s.save_macro(&mac("m2", None)).unwrap();
        s.save_macro(&mac("m3", None)).unwrap();
        assert!(s.save_macro(&mac("m4", Some("Ctrl+Alt+K"))).is_err(), "one hotkey, one macro");
        assert_eq!(s.macros().unwrap().len(), 3);
        let mut m = mac("m1", Some("Ctrl+Alt+J"));
        m.source = "await wait(5)".into();
        s.save_macro(&m).unwrap();
        assert_eq!(s.macro_by_id("m1").unwrap().unwrap().hotkey.as_deref(), Some("Ctrl+Alt+J"));
        s.save_macro(&mac("m4", Some("Ctrl+Alt+K"))).unwrap();
        s.touch_macro_run("m4", 7).unwrap();
        assert_eq!(s.macro_by_id("m4").unwrap().unwrap().last_run, Some(7));
        assert!(s.set_macro_enabled("m2", false, 9).unwrap());
        assert!(s.delete_macro("m2").unwrap());
        assert!(!s.delete_macro("m2").unwrap());
    }

    #[test]
    fn ai_keys_are_blobs_by_provider() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.ai_key_blob("0a1b2c3d").unwrap(), None);
        s.set_ai_key_blob("0a1b2c3d", &[1, 2, 3], 1).unwrap();
        s.set_ai_key_blob("0a1b2c3d", &[4, 5], 2).unwrap();
        s.set_ai_key_blob("ffffffff", &[9], 2).unwrap();
        assert_eq!(s.ai_key_blob("0a1b2c3d").unwrap(), Some(vec![4, 5]));
        assert_eq!(s.ai_key_providers().unwrap(), vec!["0a1b2c3d".to_string(), "ffffffff".to_string()]);
        s.delete_ai_key("0a1b2c3d").unwrap();
        assert_eq!(s.ai_key_blob("0a1b2c3d").unwrap(), None);
        // Keys never reach the settings record.
        assert!(!s.load_settings().unwrap().validate().is_err());
        assert!(s.kv_get("settings").unwrap().is_none_or(|j| !j.contains("ffffffff")));
    }

    #[test]
    fn ids() {
        let a = new_id();
        assert_eq!(a.len(), 32);
        assert!(valid_id(&a) && a != new_id());
        assert!(!valid_id("") && !valid_id("A") && !valid_id("a b") && !valid_id(&"a".repeat(65)));
    }

    /// A 0.11.7 database (schema 6, the 0.8.0 fixture migrated and filled) gains the 0.12.0
    /// tables without losing anything; opening again is a no-op.
    #[test]
    fn a_0_11_7_database_migrates_to_v7() {
        let dir = std::env::temp_dir().join(format!("lb-fixture-v6-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("browser.db");
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/storage/browser-v5-0.8.0.db");
        std::fs::copy(src, &path).unwrap();
        {
            let s = Store::open_at_version(&path, 6).unwrap();
            let v: i64 = s.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
            assert_eq!(v, 6);
            let t: i64 = s
                .conn
                .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'userscripts'", [], |r| r.get(0))
                .unwrap();
            assert_eq!(t, 0, "schema 6 has no 0.12.0 tables");
            s.kv_set("marker", "0.11.7").unwrap();
        }
        let s = Store::open(&path).unwrap();
        let v: i64 = s.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 7);
        assert_eq!(s.kv_get("marker").unwrap().as_deref(), Some("0.11.7"));
        let n: i64 = s.conn.query_row("SELECT count(*) FROM bookmarks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 11);
        assert!(s.userscripts().unwrap().is_empty() && s.macros().unwrap().is_empty());
        s.save_userscript(&script("x", "X")).unwrap();
        drop(s);
        let s = Store::open(&path).unwrap();
        assert_eq!(s.userscripts().unwrap().len(), 1);
        // No sync triggers on the new tables.
        let trig: i64 = s
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND (tbl_name IN ('userscripts','userscript_values','macros','ai_keys'))",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(trig, 0);
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
