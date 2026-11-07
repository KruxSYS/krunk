//! Merging imported data into Krunk's own stores without duplicates: history by (url, visit
//! time), bookmarks with the Netscape-import rules (same folder and URL), logins by origin
//! and username (a different password updates the entry, the same one is already there).

use zeroize::Zeroizing;

use super::{Login, Visit};
use crate::storage::{DbResult, Store};
use crate::vault::{Entry, KeyWrapper, VResult, Vault, VaultError};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct VisitCounts {
    pub added: usize,
    pub skipped: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LoginCounts {
    pub added: usize,
    pub updated: usize,
    /// The same login with the same password is already in the vault.
    pub merged: usize,
    /// Not a login Krunk can keep (no web origin, no password, too long) — 0.7.0: counted
    /// apart from `merged`, which 0.6.0 reported as "already there" too.
    pub invalid: usize,
}

impl Store {
    /// Adds the visits that are not there yet (same URL and time).
    pub fn import_visits(&mut self, visits: &[Visit]) -> DbResult<VisitCounts> {
        let mut counts = VisitCounts::default();
        let tx = self.conn.unchecked_transaction()?;
        for v in visits {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM visits x JOIN urls u ON u.id = x.url_id WHERE u.url = ?1 AND x.ts = ?2)",
                rusqlite::params![v.url, v.ts_ms],
                |r| r.get(0),
            )?;
            if exists {
                counts.skipped += 1;
                continue;
            }
            match self.record_visit(&v.url, &v.title, v.ts_ms)? {
                Some(_) => counts.added += 1,
                None => counts.skipped += 1,
            }
        }
        tx.commit()?;
        Ok(counts)
    }
}

impl Vault {
    /// Merges logins: a new origin + username is added, a known one with another password
    /// is updated, the same password is skipped. Saves once at the end.
    pub fn import_logins(&mut self, logins: Vec<Login>, now: i64, wrapper: &dyn KeyWrapper) -> VResult<LoginCounts> {
        let mut counts = LoginCounts::default();
        let mut changed = false;
        for login in logins {
            let Some(origin) = crate::urlclean::origin_of(&login.url) else {
                counts.invalid += 1;
                continue;
            };
            if login.password.is_empty()
                || login.username.chars().count() > 512
                || login.password.chars().count() > 1024
            {
                counts.invalid += 1;
                continue;
            }
            match self.entries.iter_mut().find(|e| e.origin == origin && e.username == login.username) {
                Some(e) if *e.password == *login.password => counts.merged += 1,
                Some(e) => {
                    e.password = login.password;
                    e.updated = now;
                    counts.updated += 1;
                    changed = true;
                }
                None => {
                    let created = login.created_ms.filter(|t| *t > 0 && *t <= now).unwrap_or(now);
                    self.entries.push(Entry {
                        id: crate::vault::random_id().map_err(|_| VaultError::InvalidEntry("id"))?,
                        origin,
                        username: login.username,
                        password: login.password,
                        note: Zeroizing::new(login.note.chars().take(2048).collect()),
                        created,
                        updated: now,
                    });
                    counts.added += 1;
                    changed = true;
                }
            }
        }
        if changed {
            self.save(wrapper)?;
        }
        Ok(counts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netscape::ImportNode;
    use crate::vault::{KdfParams, NoWrap};
    use std::path::PathBuf;

    fn visit(url: &str, ts: i64) -> Visit {
        Visit { url: url.into(), title: "T".into(), ts_ms: ts }
    }

    #[test]
    fn visits_merge_without_duplicates() {
        let mut s = Store::open_in_memory().unwrap();
        s.record_visit("https://a.example/", "A", 1000).unwrap();
        let visits = [
            visit("https://a.example/", 1000),
            visit("https://a.example/", 2000),
            visit("https://b.example/", 3000),
            visit("chrome://x/", 4000),
        ];
        let c = s.import_visits(&visits).unwrap();
        assert_eq!(c, VisitCounts { added: 2, skipped: 2 });
        let again = s.import_visits(&visits).unwrap();
        assert_eq!(again, VisitCounts { added: 0, skipped: 4 });
        assert_eq!(s.visit_total().unwrap(), 3);
    }

    #[test]
    fn bookmark_nodes_merge_without_duplicates() {
        let s = Store::open_in_memory().unwrap();
        let nodes = vec![
            ImportNode::Folder {
                title: "bar".into(),
                toolbar: true,
                add_date: 1,
                children: vec![ImportNode::Link { title: "A".into(), url: "https://a.example/".into(), add_date: 1 }],
            },
            ImportNode::Link { title: "B".into(), url: "https://b.example/".into(), add_date: 2 },
        ];
        let first = s.import_bookmark_nodes(&nodes).unwrap();
        assert_eq!((first.bookmarks, first.folders, first.skipped), (2, 0, 0));
        let second = s.import_bookmark_nodes(&nodes).unwrap();
        assert_eq!((second.bookmarks, second.skipped), (0, 2));
    }

    struct TestDir(PathBuf);
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn login(url: &str, user: &str, pw: &str) -> Login {
        Login {
            url: url.into(),
            username: user.into(),
            password: Zeroizing::new(pw.into()),
            note: Zeroizing::new(String::new()),
            created_ms: Some(1_600_000_000_000),
        }
    }

    #[test]
    fn logins_merge_add_update_skip() {
        let dir = TestDir(std::env::temp_dir().join(format!("lb-import-merge-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&dir.0);
        std::fs::create_dir_all(&dir.0).unwrap();
        let path = dir.0.join("vault.json");
        let mut v = Vault::create(&path, "correct horse battery", KdfParams::insecure_for_tests(), &NoWrap).unwrap();
        v.save_login("https://a.example/x", "alice", Zeroizing::new("old".into()), 5, &NoWrap).unwrap();
        let batch = vec![
            login("https://a.example/", "alice", "new"), // update
            login("https://b.example/", "bob", "b"),     // add
            login("https://b.example/", "bob", "b"),     // duplicate in the batch: skip
            login("not a url", "x", "y"),                // skip
            login("https://c.example/", "carol", ""),    // empty password: skip
        ];
        let c = v.import_logins(batch, 1_700_000_000_000, &NoWrap).unwrap();
        assert_eq!(c, LoginCounts { added: 1, updated: 1, merged: 1, invalid: 2 });
        let alice = v.for_origin("https://a.example/").into_iter().find(|e| e.username == "alice").unwrap();
        assert_eq!(alice.password.as_str(), "new");
        assert_eq!(alice.updated, 1_700_000_000_000);
        let bob = v.for_origin("https://b.example/")[0];
        assert_eq!(bob.created, 1_600_000_000_000, "the source's creation time is kept");
        // Reopened: persisted; a second import changes nothing.
        let mut again = Vault::open(&path, "correct horse battery", &NoWrap).unwrap();
        assert_eq!(again.entries().len(), 2);
        let c2 =
            again.import_logins(vec![login("https://b.example/", "bob", "b")], 1_700_000_000_001, &NoWrap).unwrap();
        assert_eq!(c2, LoginCounts { added: 0, updated: 0, merged: 1, invalid: 0 });
    }
}
