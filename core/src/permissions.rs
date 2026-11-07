//! Site permissions Krunk decides itself (Krunk 0.5.0): local network access. Chromium asks
//! the embedder before a public page may reach a private address or this computer
//! (`CEF_PERMISSION_TYPE_LOCAL_NETWORK` / `LOOPBACK_NETWORK`); without an answer the request
//! waits forever. The user's choice is kept per origin in `browser.db` (never for private
//! tabs) and shown and removable on krunk://sitedata.

use rusqlite::{params, OptionalExtension};

use crate::storage::{DbError, DbResult, Store};

/// The permission kind of local network access (its stored key; see [`Kind`]).
pub const LOCAL_NETWORK: &str = "localNetwork";

/// The permissions Krunk asks about itself (Krunk 0.6.0). Each is stored per site under
/// its `key()`; the defaults (Ask or Block) are in the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Camera,
    Microphone,
    Geolocation,
    Notifications,
    ClipboardRead,
    Midi,
    ScreenShare,
    LocalNetwork,
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::Camera,
        Kind::Microphone,
        Kind::Geolocation,
        Kind::Notifications,
        Kind::ClipboardRead,
        Kind::Midi,
        Kind::ScreenShare,
        Kind::LocalNetwork,
    ];

    /// The stored key (also the settings-page field name).
    pub fn key(self) -> &'static str {
        match self {
            Kind::Camera => "camera",
            Kind::Microphone => "microphone",
            Kind::Geolocation => "geolocation",
            Kind::Notifications => "notifications",
            Kind::ClipboardRead => "clipboardRead",
            Kind::Midi => "midi",
            Kind::ScreenShare => "screenShare",
            Kind::LocalNetwork => LOCAL_NETWORK,
        }
    }

    pub fn from_key(key: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key() == key)
    }

    /// The prompt's wording ("<site> wants to …").
    pub fn wants(self) -> &'static str {
        match self {
            Kind::Camera => "use your camera",
            Kind::Microphone => "use your microphone",
            Kind::Geolocation => "know your location",
            Kind::Notifications => "show notifications",
            Kind::ClipboardRead => "read text and images you copied",
            Kind::Midi => "control your MIDI devices",
            Kind::ScreenShare => "see your screen",
            Kind::LocalNetwork => "connect to devices on your local network",
        }
    }

    /// The settings page's label.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Camera => "Camera",
            Kind::Microphone => "Microphone",
            Kind::Geolocation => "Location",
            Kind::Notifications => "Notifications",
            Kind::ClipboardRead => "Clipboard (read)",
            Kind::Midi => "MIDI devices",
            Kind::ScreenShare => "Screen sharing",
            Kind::LocalNetwork => "Local network",
        }
    }
}

/// CEF's `cef_permission_request_types_t` bits Krunk answers.
pub const BIT_CAMERA_STREAM: u32 = 1 << 2;
pub const BIT_CLIPBOARD: u32 = 1 << 4;
pub const BIT_GEOLOCATION: u32 = 1 << 8;
pub const BIT_MIC_STREAM: u32 = 1 << 12;
pub const BIT_MIDI_SYSEX: u32 = 1 << 13;
pub const BIT_NOTIFICATIONS: u32 = 1 << 15;

/// The kinds a permission prompt's bits ask for, or the bits Krunk does not know (such a
/// prompt is denied rather than left hanging).
pub fn kinds_from_prompt_bits(bits: u32) -> Result<Vec<Kind>, u32> {
    let table: [(u32, Kind); 9] = [
        (BIT_CAMERA_STREAM, Kind::Camera),
        (BIT_MIC_STREAM, Kind::Microphone),
        (BIT_GEOLOCATION, Kind::Geolocation),
        (BIT_NOTIFICATIONS, Kind::Notifications),
        (BIT_CLIPBOARD, Kind::ClipboardRead),
        (BIT_MIDI_SYSEX, Kind::Midi),
        (BIT_LOCAL_NETWORK_DEPRECATED, Kind::LocalNetwork),
        (BIT_LOCAL_NETWORK, Kind::LocalNetwork),
        (BIT_LOOPBACK_NETWORK, Kind::LocalNetwork),
    ];
    let known: u32 = table.iter().map(|(b, _)| b).fold(0, |a, b| a | b);
    if bits == 0 || bits & !known != 0 {
        return Err(bits & !known);
    }
    let mut out = Vec::new();
    for (b, k) in table {
        if bits & b != 0 && !out.contains(&k) {
            out.push(k);
        }
    }
    Ok(out)
}

/// CEF's `cef_media_access_permission_types_t` bits (`getUserMedia`, `getDisplayMedia`).
pub const MEDIA_DEVICE_AUDIO: u32 = 1 << 0;
pub const MEDIA_DEVICE_VIDEO: u32 = 1 << 1;
pub const MEDIA_DESKTOP_AUDIO: u32 = 1 << 2;
pub const MEDIA_DESKTOP_VIDEO: u32 = 1 << 3;

/// The kinds a media access request asks for.
pub fn kinds_from_media_bits(bits: u32) -> Vec<Kind> {
    let mut out = Vec::new();
    if bits & MEDIA_DEVICE_VIDEO != 0 {
        out.push(Kind::Camera);
    }
    if bits & MEDIA_DEVICE_AUDIO != 0 {
        out.push(Kind::Microphone);
    }
    if bits & (MEDIA_DESKTOP_VIDEO | MEDIA_DESKTOP_AUDIO) != 0 {
        out.push(Kind::ScreenShare);
    }
    out
}

/// What a site's permission is set to (the lock popup and the settings page).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionState {
    Ask,
    Allow,
    Block,
}

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

    /// Forgets every decision of one origin (clearing its site data).
    pub fn permissions_clear_origin(&self, origin: &str) -> DbResult<usize> {
        let Some(o) = crate::urlclean::origin_of(origin) else { return Ok(0) };
        self.conn.execute("DELETE FROM site_permissions WHERE origin = ?1", [o])
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
    fn kinds_and_bits() {
        assert_eq!(
            kinds_from_prompt_bits(BIT_CAMERA_STREAM | BIT_MIC_STREAM),
            Ok(vec![Kind::Camera, Kind::Microphone])
        );
        assert_eq!(kinds_from_prompt_bits(BIT_GEOLOCATION), Ok(vec![Kind::Geolocation]));
        assert_eq!(kinds_from_prompt_bits(BIT_LOCAL_NETWORK | BIT_LOOPBACK_NETWORK), Ok(vec![Kind::LocalNetwork]));
        assert_eq!(kinds_from_prompt_bits(0), Err(0));
        assert_eq!(kinds_from_prompt_bits(1 << 20), Err(1 << 20), "an unknown type is reported");
        assert_eq!(kinds_from_prompt_bits(BIT_CLIPBOARD | 1 << 20), Err(1 << 20));
        assert_eq!(
            kinds_from_media_bits(MEDIA_DEVICE_VIDEO | MEDIA_DEVICE_AUDIO),
            vec![Kind::Camera, Kind::Microphone]
        );
        assert_eq!(kinds_from_media_bits(MEDIA_DESKTOP_VIDEO), vec![Kind::ScreenShare]);
        assert!(kinds_from_media_bits(0).is_empty());
        for k in Kind::ALL {
            assert_eq!(Kind::from_key(k.key()), Some(k));
            assert!(!k.wants().is_empty() && !k.label().is_empty());
        }
        assert_eq!(Kind::LocalNetwork.key(), LOCAL_NETWORK);
        assert_eq!(serde_json::to_value(Kind::ClipboardRead).unwrap(), serde_json::json!("clipboardRead"));
        assert_eq!(serde_json::to_value(PermissionState::Block).unwrap(), serde_json::json!("block"));
    }

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
        s.permission_set("https://a.example/", LOCAL_NETWORK, true, 12).unwrap();
        s.permission_set("https://a.example/", "camera", false, 12).unwrap();
        s.permission_set("https://b.example/", "camera", false, 12).unwrap();
        assert_eq!(
            s.permissions_clear_origin("https://a.example/x?y").unwrap(),
            2,
            "clearing an origin's site data forgets its answers"
        );
        assert_eq!(s.permissions_clear_origin("nonsense").unwrap(), 0);
        assert_eq!(s.permissions_list().unwrap().len(), 2);
        s.permissions_clear().unwrap();
        assert!(s.permissions_list().unwrap().is_empty());
    }
}
