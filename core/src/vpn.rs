//! The in-browser VPN (0.14.0): the stored profiles (`vpn_profiles`), what Settings, the shell
//! and the popup are told, and the checks of what the pages send.
//!
//! Only Krunk's own connections use the VPN: the host runs the `krunk_vpn` crate's local proxy
//! and points the engine and its own HTTP at it. A profile's file and its credentials are
//! stored as one blob wrapped by the host (DPAPI with its own entropy); this module only sees
//! them through a [`KeyWrapper`], in zeroizing memory. The table has no sync triggers and is
//! never part of settings export, import or reset.

use crate::storage::{DbResult, Store};
use crate::vault::KeyWrapper;
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub(crate) const SCHEMA_V8: &str = "
CREATE TABLE vpn_profiles (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    kind              TEXT NOT NULL,
    server            TEXT NOT NULL,
    needs_credentials INTEGER NOT NULL,
    created_ms        INTEGER NOT NULL,
    blob              BLOB NOT NULL
);
";

/// At most this many profiles; an imported file is at most this large.
pub const MAX_PROFILES: usize = 32;
pub const MAX_IMPORT_BYTES: usize = 64 * 1024;
pub const MAX_NAME_CHARS: usize = 60;
pub const MAX_USERNAME_CHARS: usize = 256;
pub const MAX_PASSWORD_CHARS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VpnKind {
    WireGuard,
    OpenVpn,
}

impl VpnKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VpnKind::WireGuard => "wireguard",
            VpnKind::OpenVpn => "openvpn",
        }
    }

    pub fn parse(s: &str) -> Option<VpnKind> {
        match s {
            "wireguard" => Some(VpnKind::WireGuard),
            "openvpn" => Some(VpnKind::OpenVpn),
            _ => None,
        }
    }
}

/// A stored profile without its secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnProfile {
    pub id: String,
    pub name: String,
    pub kind: VpnKind,
    /// `host:port` of the first remote or the peer's endpoint.
    pub server: String,
    /// OpenVPN `auth-user-pass` without an inline pair: a user name and password are needed.
    pub needs_credentials: bool,
    pub created_ms: i64,
}

/// A profile as the UI shows it: `has_credentials` says whether a user name and password are
/// stored with it (never what they are).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnProfileView {
    pub id: String,
    pub name: String,
    pub kind: VpnKind,
    pub server: String,
    pub needs_credentials: bool,
    pub has_credentials: bool,
}

impl VpnProfileView {
    pub fn new(p: &VpnProfile, has_credentials: bool) -> VpnProfileView {
        VpnProfileView {
            id: p.id.clone(),
            name: p.name.clone(),
            kind: p.kind,
            server: p.server.clone(),
            needs_credentials: p.needs_credentials,
            has_credentials,
        }
    }
}

/// The VPN's state (the `krunk_vpn::Status` the host passes on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VpnState {
    Connecting,
    Connected,
    Reconnecting,
    Failed,
    #[default]
    Stopped,
}

impl VpnState {
    /// The proxy is in place (the engine's traffic goes to it, or nowhere): every state but
    /// `Stopped`.
    pub fn engaged(self) -> bool {
        self != VpnState::Stopped
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnStatus {
    pub state: VpnState,
    /// Human text for `failed` and `reconnecting`.
    pub error: Option<String>,
    /// When the current state began (unix ms).
    pub since_ms: i64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// Our address inside the tunnel.
    pub tunnel_ip: Option<String>,
    pub server: String,
}

/// What the VPN card, the toolbar button and the popup show.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnView {
    pub status: VpnStatus,
    pub profiles: Vec<VpnProfileView>,
    /// The profile the VPN runs with, or else the one used last (`vpn.active_profile`).
    pub active: String,
    pub show_button: bool,
}

/// What the blob holds: the profile file (after the import rewrite) and the OpenVPN user name
/// and password, if stored.
pub struct ProfileSecret {
    pub text: Zeroizing<String>,
    pub username: Option<Zeroizing<String>>,
    pub password: Option<Zeroizing<String>>,
}

impl ProfileSecret {
    pub fn has_credentials(&self) -> bool {
        self.username.as_ref().is_some_and(|u| !u.is_empty())
    }
}

impl std::fmt::Debug for ProfileSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProfileSecret(<redacted>)")
    }
}

#[derive(Serialize)]
struct SecretOut<'a> {
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<&'a str>,
}

#[derive(Deserialize)]
struct SecretIn {
    text: Zeroizing<String>,
    #[serde(default)]
    username: Option<Zeroizing<String>>,
    #[serde(default)]
    password: Option<Zeroizing<String>>,
}

/// The JSON `{text, username?, password?}`, wrapped by `wrapper`.
pub fn seal(wrapper: &dyn KeyWrapper, secret: &ProfileSecret) -> Result<Vec<u8>, String> {
    let out = SecretOut {
        text: &secret.text,
        username: secret.username.as_deref().map(String::as_str),
        password: secret.password.as_deref().map(String::as_str),
    };
    // Room for the escapes up front, so the buffer is not reallocated (and copied) on the way.
    let room = 64 + 2 * (secret.text.len() + out.username.map_or(0, str::len) + out.password.map_or(0, str::len));
    let mut json = Zeroizing::new(Vec::with_capacity(room));
    serde_json::to_writer(&mut *json, &out).map_err(|e| e.to_string())?;
    wrapper.protect(&json)
}

/// Unwraps and parses a blob made by [`seal`].
pub fn open(wrapper: &dyn KeyWrapper, blob: &[u8]) -> Result<ProfileSecret, String> {
    let json = wrapper.unprotect(blob)?;
    let s: SecretIn = serde_json::from_slice(&json).map_err(|_| "The stored profile is damaged".to_string())?;
    Ok(ProfileSecret { text: s.text, username: s.username, password: s.password })
}

/// A profile name: trimmed, 1 to 60 characters, no control characters.
pub fn clean_name(name: &str) -> Result<String, String> {
    let t = name.trim();
    if t.is_empty() {
        return Err("Enter a name".into());
    }
    if t.chars().count() > MAX_NAME_CHARS || t.chars().any(char::is_control) {
        return Err("A name has at most 60 characters, without line breaks".into());
    }
    Ok(t.to_string())
}

/// The name an imported file gets: its file name without the extension (or "VPN").
pub fn name_from_file(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = match base.rsplit_once('.') {
        Some((s, ext)) if !s.is_empty() && matches!(ext.to_ascii_lowercase().as_str(), "conf" | "ovpn") => s,
        _ => base,
    };
    let clean: String = stem.chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect();
    if clean.trim().is_empty() {
        "VPN".into()
    } else {
        clean.trim().to_string()
    }
}

/// OpenVPN credentials: a user name (1–256 characters) and a password (up to 1024), neither
/// with line breaks or other control characters (OpenVPN's auth file is line based).
pub fn check_credentials(username: &str, password: &str) -> Result<(), String> {
    if username.trim().is_empty() {
        return Err("Enter the user name".into());
    }
    if username.chars().count() > MAX_USERNAME_CHARS || username.chars().any(char::is_control) {
        return Err("The user name is too long or has line breaks".into());
    }
    if password.chars().count() > MAX_PASSWORD_CHARS || password.chars().any(char::is_control) {
        return Err("The password is too long or has line breaks".into());
    }
    Ok(())
}

pub use crate::scripts_store::{new_id, valid_id};

const COLS: &str = "id, name, kind, server, needs_credentials, created_ms";

fn row(r: &Row) -> rusqlite::Result<VpnProfile> {
    let kind: String = r.get(2)?;
    Ok(VpnProfile {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: VpnKind::parse(&kind).unwrap_or(VpnKind::WireGuard),
        server: r.get(3)?,
        needs_credentials: r.get(4)?,
        created_ms: r.get(5)?,
    })
}

impl Store {
    /// Every profile, oldest first.
    pub fn vpn_profiles(&self) -> DbResult<Vec<VpnProfile>> {
        let mut st = self.conn.prepare(&format!("SELECT {COLS} FROM vpn_profiles ORDER BY created_ms, id"))?;
        let rows = st.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn vpn_profile(&self, id: &str) -> DbResult<Option<VpnProfile>> {
        self.conn.query_row(&format!("SELECT {COLS} FROM vpn_profiles WHERE id = ?1"), [id], row).optional()
    }

    pub fn vpn_profile_blob(&self, id: &str) -> DbResult<Option<Vec<u8>>> {
        self.conn.query_row("SELECT blob FROM vpn_profiles WHERE id = ?1", [id], |r| r.get(0)).optional()
    }

    pub fn vpn_profile_add(&self, p: &VpnProfile, blob: &[u8]) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO vpn_profiles(id, name, kind, server, needs_credentials, created_ms, blob)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![p.id, p.name, p.kind.as_str(), p.server, p.needs_credentials, p.created_ms, blob],
        )?;
        Ok(())
    }

    /// False when there is no such profile.
    pub fn vpn_profile_rename(&self, id: &str, name: &str) -> DbResult<bool> {
        Ok(self.conn.execute("UPDATE vpn_profiles SET name = ?2 WHERE id = ?1", params![id, name])? > 0)
    }

    pub fn vpn_profile_set_blob(&self, id: &str, blob: &[u8]) -> DbResult<bool> {
        Ok(self.conn.execute("UPDATE vpn_profiles SET blob = ?2 WHERE id = ?1", params![id, blob])? > 0)
    }

    pub fn vpn_profile_remove(&self, id: &str) -> DbResult<bool> {
        Ok(self.conn.execute("DELETE FROM vpn_profiles WHERE id = ?1", [id])? > 0)
    }

    pub fn vpn_profile_count(&self) -> DbResult<usize> {
        let n: i64 = self.conn.query_row("SELECT count(*) FROM vpn_profiles", [], |r| r.get(0))?;
        Ok(n as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wrapper that only marks and reverses the bytes (the host's is DPAPI).
    struct Fake;
    impl KeyWrapper for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
            let mut v: Vec<u8> = data.iter().rev().map(|b| b ^ 0x5a).collect();
            v.insert(0, 0xfe);
            Ok(v)
        }
        fn unprotect(&self, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
            match data.split_first() {
                Some((0xfe, rest)) => Ok(Zeroizing::new(rest.iter().rev().map(|b| b ^ 0x5a).collect())),
                _ => Err("not ours".into()),
            }
        }
    }

    fn profile(id: &str, created: i64) -> VpnProfile {
        VpnProfile {
            id: id.into(),
            name: "Office".into(),
            kind: VpnKind::OpenVpn,
            server: "vpn.example.org:1194".into(),
            needs_credentials: true,
            created_ms: created,
        }
    }

    #[test]
    fn profiles_round_trip_with_their_sealed_secret() {
        let s = Store::open_in_memory().unwrap();
        assert!(s.vpn_profiles().unwrap().is_empty());
        let secret = ProfileSecret {
            text: Zeroizing::new("client\nremote vpn.example.org 1194\n# \"quoted\" \\ é\n".into()),
            username: None,
            password: None,
        };
        let blob = seal(&Fake, &secret).unwrap();
        assert!(!blob.windows(6).any(|w| w == b"remote"), "the blob is wrapped");
        s.vpn_profile_add(&profile("b", 2), &blob).unwrap();
        s.vpn_profile_add(&VpnProfile { kind: VpnKind::WireGuard, needs_credentials: false, ..profile("a", 1) }, &blob)
            .unwrap();
        let list = s.vpn_profiles().unwrap();
        assert_eq!(list.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(list[0].kind, VpnKind::WireGuard);
        assert_eq!(s.vpn_profile("b").unwrap(), Some(profile("b", 2)));
        let back = open(&Fake, &s.vpn_profile_blob("b").unwrap().unwrap()).unwrap();
        assert_eq!(back.text.as_str(), secret.text.as_str());
        assert!(!back.has_credentials());

        // Credentials go into the same blob.
        let with = ProfileSecret {
            text: back.text.clone(),
            username: Some(Zeroizing::new("alice".into())),
            password: Some(Zeroizing::new("p\"w\\d".into())),
        };
        assert!(s.vpn_profile_set_blob("b", &seal(&Fake, &with).unwrap()).unwrap());
        let back = open(&Fake, &s.vpn_profile_blob("b").unwrap().unwrap()).unwrap();
        assert!(back.has_credentials());
        assert_eq!(back.password.as_deref().map(String::as_str), Some("p\"w\\d"));
        assert!(!format!("{back:?}").contains("alice"));

        assert!(s.vpn_profile_rename("b", "Home").unwrap());
        assert_eq!(s.vpn_profile("b").unwrap().unwrap().name, "Home");
        assert!(!s.vpn_profile_rename("zz", "x").unwrap());
        assert_eq!(s.vpn_profile_count().unwrap(), 2);
        assert!(s.vpn_profile_remove("a").unwrap());
        assert!(!s.vpn_profile_remove("a").unwrap());
        assert_eq!(s.vpn_profile_count().unwrap(), 1);
        // A blob of another wrapper (or damaged) is refused, never misread.
        assert!(open(&Fake, b"\x01garbage").is_err());
        assert!(open(&Fake, &Fake.protect(b"not json").unwrap()).is_err());
    }

    #[test]
    fn a_0_13_database_gets_the_table() {
        let dir = std::env::temp_dir().join(format!("krunk-vpn-migrate-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("browser.db");
        {
            let s = Store::open_at_version(&path, 7).unwrap();
            let t: i64 = s
                .conn
                .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'vpn_profiles'", [], |r| r.get(0))
                .unwrap();
            assert_eq!(t, 0, "schema 7 has no VPN table");
            s.kv_set("marker", "0.13.0").unwrap();
        }
        let s = Store::open(&path).unwrap();
        let v: i64 = s.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 8);
        assert_eq!(s.kv_get("marker").unwrap().as_deref(), Some("0.13.0"));
        s.vpn_profile_add(&profile("a", 1), b"x").unwrap();
        drop(s);
        assert_eq!(Store::open(&path).unwrap().vpn_profiles().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_table_has_no_sync_triggers() {
        let s = Store::open_in_memory().unwrap();
        let n: i64 = s
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'vpn_profiles'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn names_and_credentials() {
        assert_eq!(clean_name("  Office  ").unwrap(), "Office");
        assert!(clean_name("   ").is_err());
        assert!(clean_name("a\nb").is_err());
        assert!(clean_name(&"x".repeat(61)).is_err());
        assert_eq!(name_from_file(r"C:\Users\me\Downloads\office-vpn.ovpn"), "office-vpn");
        assert_eq!(name_from_file("wg0.conf"), "wg0");
        assert_eq!(name_from_file("noext"), "noext");
        assert_eq!(name_from_file(".conf"), ".conf");
        assert_eq!(name_from_file(""), "VPN");
        assert!(check_credentials("alice", "secret").is_ok());
        assert!(check_credentials("alice", "").is_ok());
        assert!(check_credentials(" ", "x").is_err());
        assert!(check_credentials("alice", "a\nb").is_err());
        assert!(check_credentials("al\rice", "x").is_err());
        assert!(check_credentials("alice", &"x".repeat(1025)).is_err());
    }

    #[test]
    fn view_shapes() {
        let v = VpnView {
            status: VpnStatus { state: VpnState::Connected, since_ms: 5, server: "h:1".into(), ..Default::default() },
            profiles: vec![VpnProfileView::new(&profile("b", 2), true)],
            active: "b".into(),
            show_button: true,
        };
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["status"]["state"], "connected");
        assert_eq!(j["status"]["sinceMs"], 5);
        assert_eq!(j["status"]["tunnelIp"], serde_json::Value::Null);
        assert_eq!(j["profiles"][0]["kind"], "openvpn");
        assert_eq!(j["profiles"][0]["needsCredentials"], true);
        assert_eq!(j["profiles"][0]["hasCredentials"], true);
        assert_eq!(j["showButton"], true);
        assert!(VpnState::Failed.engaged() && !VpnState::Stopped.engaged());
        assert_eq!(VpnKind::parse("openvpn"), Some(VpnKind::OpenVpn));
        assert_eq!(VpnKind::parse("OpenVPN"), None);
    }
}
