//! Site permissions Krunk decides itself (Krunk 0.5.0): local network access. Chromium asks
//! the embedder before a public page may reach a private address or this computer
//! (`CEF_PERMISSION_TYPE_LOCAL_NETWORK` / `LOOPBACK_NETWORK`); without an answer the request
//! waits forever. The user's choice is kept per origin in `browser.db` (never for private
//! tabs) and shown and removable on krunk://sitedata.

use rusqlite::{params, OptionalExtension};

use crate::storage::{DbError, DbResult, Store};

/// The one permission kind stored so far.
pub const LOCAL_NETWORK: &str = "localNetwork";

/// A stored decision.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SitePermission {
    pub origin: String,
    pub kind: String,
    pub allow: bool,
}

/// What the page asked for, from the engine's request bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkRequest {
    /// This computer (localhost) rather than another device on the local network.
    pub loopback: bool,
}

/// `CEF_PERMISSION_TYPE_LOCAL_NETWORK_ACCESS_DEPRECATED`, `_LOCAL_NETWORK`, `_LOOPBACK_NETWORK`.
pub const BIT_LOCAL_NETWORK_DEPRECATED: u32 = 1 << 25;
pub const BIT_LOCAL_NETWORK: u32 = 1 << 26;
pub const BIT_LOOPBACK_NETWORK: u32 = 1 << 27;

/// The network request in a permission prompt's bits, or `None` when the prompt asks for
/// anything else as well (those stay with the engine's default).
pub fn network_request(bits: u32) -> Option<NetworkRequest> {
    let known = BIT_LOCAL_NETWORK_DEPRECATED | BIT_LOCAL_NETWORK | BIT_LOOPBACK_NETWORK;
    if bits == 0 || bits & !known != 0 {
        return None;
    }
    Some(NetworkRequest {
        loopback: bits & BIT_LOOPBACK_NETWORK != 0 && bits & (BIT_LOCAL_NETWORK | BIT_LOCAL_NETWORK_DEPRECATED) == 0,
    })
}

fn origin_key(origin: &str) -> DbResult<String> {
    crate::urlclean::origin_of(origin).ok_or_else(|| DbError::InvalidParameterName("Not an origin".into()))
}

impl Store {
    /// The stored decision for `origin` and `kind`, if any.
    pub fn permission(&self, origin: &str, kind: &str) -> DbResult<Option<bool>> {
        let Some(o) = crate::urlclean::origin_of(origin) else { return Ok(None) };
        self.conn
            .query_row("SELECT allow FROM site_permissions WHERE origin = ?1 AND kind = ?2", params![o, kind], |r| {
                r.get::<_, i64>(0).map(|v| v != 0)
            })
            .optional()
    }

    pub fn permission_set(&self, origin: &str, kind: &str, allow: bool, now: i64) -> DbResult<()> {
        let o = origin_key(origin)?;
        self.conn.execute(
            "INSERT INTO site_permissions(origin, kind, allow, added) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(origin, kind) DO UPDATE SET allow = excluded.allow, added = excluded.added",
            params![o, kind, i64::from(allow), now],
        )?;
        Ok(())
    }

    /// Removes one decision; true when there was one.
    pub fn permission_remove(&self, origin: &str, kind: &str) -> DbResult<bool> {
        let Some(o) = crate::urlclean::origin_of(origin) else { return Ok(false) };
        Ok(self.conn.execute("DELETE FROM site_permissions WHERE origin = ?1 AND kind = ?2", params![o, kind])? > 0)
    }

    /// Every decision of one origin.
    pub fn permissions_for(&self, origin: &str) -> DbResult<Vec<SitePermission>> {
        let Some(o) = crate::urlclean::origin_of(origin) else { return Ok(Vec::new()) };
        let mut stmt =
            self.conn.prepare_cached("SELECT kind, allow FROM site_permissions WHERE origin = ?1 ORDER BY kind")?;
        let rows = stmt.query_map([&o], |r| {
            Ok(SitePermission { origin: o.clone(), kind: r.get(0)?, allow: r.get::<_, i64>(1)? != 0 })
        })?;
        rows.collect()
    }

    /// Every stored decision.
    pub fn permissions_list(&self) -> DbResult<Vec<SitePermission>> {
        let mut stmt =
            self.conn.prepare_cached("SELECT origin, kind, allow FROM site_permissions ORDER BY origin, kind")?;
        let rows = stmt.query_map([], |r| {
            Ok(SitePermission { origin: r.get(0)?, kind: r.get(1)?, allow: r.get::<_, i64>(2)? != 0 })
        })?;
        rows.collect()
    }

    /// Forgets every decision (Clear browsing data → site data).
    pub fn permissions_clear(&self) -> DbResult<()> {
        self.conn.execute("DELETE FROM site_permissions", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_bits() {
        assert_eq!(network_request(BIT_LOCAL_NETWORK), Some(NetworkRequest { loopback: false }));
        assert_eq!(network_request(BIT_LOOPBACK_NETWORK), Some(NetworkRequest { loopback: true }));
        assert_eq!(network_request(BIT_LOCAL_NETWORK | BIT_LOOPBACK_NETWORK), Some(NetworkRequest { loopback: false }));
        assert_eq!(network_request(BIT_LOCAL_NETWORK_DEPRECATED), Some(NetworkRequest { loopback: false }));
        assert_eq!(network_request(0), None, "nothing asked");
        assert_eq!(network_request(1 << 3), None, "another permission type stays with the engine");
        assert_eq!(network_request(BIT_LOOPBACK_NETWORK | 1 << 3), None, "a mixed prompt too");
    }

    #[test]
    fn decisions_are_per_origin_and_removable() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.permission("https://app.plex.tv/desktop", LOCAL_NETWORK).unwrap(), None);
        s.permission_set("https://app.plex.tv/desktop/#!/x", LOCAL_NETWORK, true, 10).unwrap();
        assert_eq!(s.permission("https://app.plex.tv/", LOCAL_NETWORK).unwrap(), Some(true));
        assert_eq!(
            s.permission("https://app.plex.tv:8443/", LOCAL_NETWORK).unwrap(),
            None,
            "another port is another origin"
        );
        assert_eq!(s.permission("http://app.plex.tv/", LOCAL_NETWORK).unwrap(), None, "another scheme too");
        s.permission_set("https://app.plex.tv", LOCAL_NETWORK, false, 11).unwrap();
        assert_eq!(
            s.permission("https://app.plex.tv", LOCAL_NETWORK).unwrap(),
            Some(false),
            "the newer choice replaces the older"
        );
        s.permission_set("https://other.example", LOCAL_NETWORK, true, 12).unwrap();
        assert_eq!(
            s.permissions_for("https://app.plex.tv").unwrap(),
            vec![SitePermission { origin: "https://app.plex.tv".into(), kind: LOCAL_NETWORK.into(), allow: false }]
        );
        assert_eq!(s.permissions_list().unwrap().len(), 2);
        assert!(s.permission_remove("https://app.plex.tv", LOCAL_NETWORK).unwrap());
        assert!(!s.permission_remove("https://app.plex.tv", LOCAL_NETWORK).unwrap());
        assert_eq!(s.permission("https://app.plex.tv", LOCAL_NETWORK).unwrap(), None);
        assert!(s.permission_set("not an origin", LOCAL_NETWORK, true, 1).is_err());
        assert_eq!(s.permission("nonsense", LOCAL_NETWORK).unwrap(), None);
        s.permissions_clear().unwrap();
        assert!(s.permissions_list().unwrap().is_empty());
    }
}
