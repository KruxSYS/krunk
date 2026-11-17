//! Krunk's in-browser VPN (0.14.0): only Krunk's own traffic goes through it, never the system's.
//!
//! There is no network adapter, no route and nothing that needs admin rights. A tunnel
//! (WireGuard through boringtun's `noise`, or OpenVPN through OpenVPN 3 Core's external TUN)
//! carries IP packets; [`net`] runs a smoltcp TCP/IP stack on them; [`proxy`] is a local HTTP
//! proxy on `127.0.0.1:<random>` that the engine and Krunk's own HTTP clients use. Names are
//! resolved inside the tunnel ([`dns`]). It is fail-closed: while the tunnel is down the proxy
//! answers `503` and nothing ever goes direct.
//!
//! The crate is platform-neutral (its tests run on Linux); the Windows-only owner-PID check of
//! the proxy's peers is the caller's `peer_allowed(proxy_local, peer)` hook. Keys, user names,
//! passwords and profile texts are never logged and are zeroized when dropped.

// The only `unsafe` is the Windows `setsockopt(SO_EXCLUSIVEADDRUSE)` in `listen`.
#![deny(unsafe_code)]

pub mod dns;
mod listen;
pub mod net;
#[cfg(feature = "openvpn")]
pub mod ovpn;
pub mod proxy;
mod service;
pub mod wg;

pub use listen::bind_loopback;
pub use service::{PeerAllowed, Vpn};

use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroizing;

/// The largest file [`import`] accepts.
pub const MAX_IMPORT_BYTES: usize = 64 * 1024;

/// The kind of a profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A WireGuard `.conf` (one `[Interface]`, one `[Peer]`).
    WireGuard,
    /// An OpenVPN `.ovpn` with inline certificates.
    OpenVpn,
}

impl Kind {
    /// `"wireguard"` or `"openvpn"` (the serde names).
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::WireGuard => "wireguard",
            Kind::OpenVpn => "openvpn",
        }
    }
}

/// A parsed profile, ready to connect. Secrets are zeroized on drop.
pub enum Profile {
    WireGuard(wg::Config),
    /// The `.ovpn` text after the import rewrite, plus optional inline credentials.
    #[cfg(feature = "openvpn")]
    OpenVpn(ovpn::Profile),
}

impl Profile {
    pub fn kind(&self) -> Kind {
        match self {
            Profile::WireGuard(_) => Kind::WireGuard,
            #[cfg(feature = "openvpn")]
            Profile::OpenVpn(_) => Kind::OpenVpn,
        }
    }

    /// `host:port` of the server.
    pub fn server(&self) -> String {
        match self {
            Profile::WireGuard(c) => c.endpoint().to_owned(),
            #[cfg(feature = "openvpn")]
            Profile::OpenVpn(p) => p.server().to_owned(),
        }
    }
}

impl fmt::Debug for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Profile::WireGuard(c) => f.debug_tuple("WireGuard").field(c).finish(),
            #[cfg(feature = "openvpn")]
            Profile::OpenVpn(p) => f.debug_tuple("OpenVpn").field(p).finish(),
        }
    }
}

/// What the import shows and stores (no secrets in it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    pub kind: Kind,
    /// From the file name, or a comment in the file.
    pub name: String,
    /// `host:port` (the first remote / the peer's endpoint).
    pub server: String,
    /// OpenVPN `auth-user-pass` without an inline pair.
    pub needs_credentials: bool,
    /// Ignored options: PostUp/PreUp, DNS search domains, scripts, unsupported features.
    pub warnings: Vec<String>,
}

/// Why a file was not imported. The texts are for the user and never quote secrets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// Larger than [`MAX_IMPORT_BYTES`].
    TooLarge,
    /// Neither a WireGuard nor an OpenVPN configuration.
    Unrecognized,
    /// Recognised but broken (the text says what is wrong).
    Invalid(String),
    /// Recognised but uses something Krunk does not support.
    Unsupported(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::TooLarge => write!(f, "The file is larger than {} KiB", MAX_IMPORT_BYTES / 1024),
            ImportError::Unrecognized => f.write_str("This is not a WireGuard or OpenVPN configuration file"),
            ImportError::Invalid(m) | ImportError::Unsupported(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ImportError {}

/// The import error for an OpenVPN file in a build without the `openvpn` feature.
pub const OPENVPN_UNSUPPORTED: &str = "OpenVPN is not supported on this platform";

/// Whether this build can connect OpenVPN profiles (the `openvpn` cargo feature).
pub const OPENVPN_SUPPORTED: bool = cfg!(feature = "openvpn");

/// Parses an imported file (at most 64 KiB) of either kind; the kind is detected from the
/// content, the file name only gives the profile its name.
pub fn import(file_name: &str, text: &str) -> Result<(Profile, ProfileInfo), ImportError> {
    if text.len() > MAX_IMPORT_BYTES {
        return Err(ImportError::TooLarge);
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.contains('\0') {
        return Err(ImportError::Unrecognized);
    }
    match detect(text) {
        Some(Kind::WireGuard) => {
            let parsed = wg::parse(text)?;
            let name = profile_name(file_name, parsed.name.as_deref(), "WireGuard");
            let info = ProfileInfo {
                kind: Kind::WireGuard,
                name,
                server: parsed.config.endpoint().to_owned(),
                needs_credentials: false,
                warnings: parsed.warnings,
            };
            Ok((Profile::WireGuard(parsed.config), info))
        }
        #[cfg(not(feature = "openvpn"))]
        Some(Kind::OpenVpn) => Err(ImportError::Unsupported(OPENVPN_UNSUPPORTED.to_owned())),
        #[cfg(feature = "openvpn")]
        Some(Kind::OpenVpn) => {
            let parsed = ovpn::inspect(text)?;
            let name = profile_name(file_name, parsed.name.as_deref(), "OpenVPN");
            let info = ProfileInfo {
                kind: Kind::OpenVpn,
                name,
                server: parsed.profile.server().to_owned(),
                needs_credentials: parsed.profile.needs_credentials(),
                warnings: parsed.warnings,
            };
            Ok((Profile::OpenVpn(parsed.profile), info))
        }
        None => Err(ImportError::Unrecognized),
    }
}

/// WireGuard if there is an `[Interface]` section, OpenVPN if typical directives appear.
fn detect(text: &str) -> Option<Kind> {
    const OVPN: &[&str] = &[
        "client",
        "remote",
        "dev",
        "proto",
        "<ca>",
        "ca",
        "tls-client",
        "auth-user-pass",
        "<connection>",
        "remote-cert-tls",
        "nobind",
    ];
    let mut ovpn_hits = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.eq_ignore_ascii_case("[interface]") {
            return Some(Kind::WireGuard);
        }
        let first = line.split_ascii_whitespace().next().unwrap_or("");
        if OVPN.iter().any(|d| first.eq_ignore_ascii_case(d)) {
            ovpn_hits += 1;
        }
    }
    (ovpn_hits >= 2).then_some(Kind::OpenVpn)
}

/// The file's stem, else a `# Name = …` comment, else the kind; printable and at most 64 chars.
fn profile_name(file_name: &str, comment: Option<&str>, fallback: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = match base.rsplit_once('.') {
        Some((stem, ext))
            if ext.eq_ignore_ascii_case("conf")
                || ext.eq_ignore_ascii_case("ovpn")
                || ext.eq_ignore_ascii_case("txt") =>
        {
            stem
        }
        _ => base,
    };
    for candidate in [Some(stem), comment, Some(fallback)].into_iter().flatten() {
        let clean: String = candidate.chars().filter(|c| !c.is_control()).take(64).collect();
        let clean = clean.trim();
        if !clean.is_empty() {
            return clean.to_owned();
        }
    }
    fallback.to_owned()
}

/// OpenVPN username and password (both zeroized when dropped).
pub struct Credentials {
    pub username: Zeroizing<String>,
    pub password: Zeroizing<String>,
}

impl Clone for Credentials {
    fn clone(&self) -> Self {
        Credentials {
            username: Zeroizing::new(self.username.as_str().to_owned()),
            password: Zeroizing::new(self.password.as_str().to_owned()),
        }
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials").field("username", &"<redacted>").field("password", &"<redacted>").finish()
    }
}

/// The VPN's state, as the UI shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Connecting,
    Connected,
    Reconnecting,
    /// Gave up (refused password, certificate): no retry until started again.
    Failed,
    Stopped,
}

/// A snapshot of the running VPN.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: State,
    /// Human text for failed/reconnecting (auth failed, certificate, timeout …).
    pub error: Option<String>,
    /// When the current state began (unix ms).
    pub since_ms: i64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// Our address inside the tunnel.
    pub tunnel_ip: Option<String>,
    /// `host:port` we connect to.
    pub server: String,
}

/// Why [`Vpn::start`] failed.
#[derive(Debug)]
pub enum StartError {
    /// The OpenVPN profile needs a username and password and none were given.
    NeedsCredentials,
    /// The runtime or the proxy listener could not be created.
    Io(std::io::Error),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::NeedsCredentials => f.write_str("This profile needs a username and password"),
            StartError::Io(e) => write!(f, "The VPN could not start: {e}"),
        }
    }
}

impl std::error::Error for StartError {}

impl From<std::io::Error> for StartError {
    fn from(e: std::io::Error) -> Self {
        StartError::Io(e)
    }
}

/// A network and prefix length (`10.0.0.0/8`), used for addresses, AllowedIPs and routes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IpNet {
    pub addr: std::net::IpAddr,
    pub prefix: u8,
}

impl IpNet {
    /// `addr/prefix`, or `None` when the prefix is too long for the family.
    pub fn new(addr: std::net::IpAddr, prefix: u8) -> Option<IpNet> {
        let max = if addr.is_ipv4() { 32 } else { 128 };
        (prefix <= max).then_some(IpNet { addr, prefix })
    }

    /// Parses `a.b.c.d/n`, `x::y/n` or a bare address (a host route).
    pub fn parse(s: &str) -> Option<IpNet> {
        let s = s.trim();
        let (addr, prefix) = match s.split_once('/') {
            Some((a, p)) => {
                if p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                (a.parse().ok()?, p.parse().ok()?)
            }
            None => {
                let a: std::net::IpAddr = s.parse().ok()?;
                (a, if a.is_ipv4() { 32 } else { 128 })
            }
        };
        IpNet::new(addr, prefix)
    }

    pub fn contains(&self, ip: std::net::IpAddr) -> bool {
        match (self.addr, ip) {
            (std::net::IpAddr::V4(n), std::net::IpAddr::V4(a)) => {
                let mask = if self.prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(self.prefix)) };
                u32::from(n) & mask == u32::from(a) & mask
            }
            (std::net::IpAddr::V6(n), std::net::IpAddr::V6(a)) => {
                let mask = if self.prefix == 0 { 0 } else { u128::MAX << (128 - u32::from(self.prefix)) };
                u128::from(n) & mask == u128::from(a) & mask
            }
            _ => false,
        }
    }
}

impl fmt::Display for IpNet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

/// Unix time in milliseconds.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Random bytes from the OS (getrandom); a failure here is not recoverable.
pub(crate) fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the OS random number generator failed");
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_kinds() {
        assert_eq!(detect("[Interface]\nPrivateKey = x\n"), Some(Kind::WireGuard));
        assert_eq!(detect("# c\n  [interface]  \n"), Some(Kind::WireGuard));
        assert_eq!(detect("client\nremote a 1194\n"), Some(Kind::OpenVpn));
        assert_eq!(detect("hello\nworld\n"), None);
        assert_eq!(detect("remote x\n"), None);
    }

    #[test]
    fn import_rejects_large_and_garbage() {
        let big = "a".repeat(MAX_IMPORT_BYTES + 1);
        assert_eq!(import("x.conf", &big).unwrap_err(), ImportError::TooLarge);
        assert_eq!(import("x.conf", "just text").unwrap_err(), ImportError::Unrecognized);
        assert_eq!(import("x.conf", "[Interface]\0").unwrap_err(), ImportError::Unrecognized);
    }

    #[cfg(not(feature = "openvpn"))]
    #[test]
    fn openvpn_unsupported_without_the_feature() {
        let e = import("x.ovpn", "client\nremote vpn.example 1194\n").unwrap_err();
        assert_eq!(e, ImportError::Unsupported(OPENVPN_UNSUPPORTED.to_owned()));
    }

    #[test]
    fn names() {
        assert_eq!(profile_name("C:\\Users\\a\\Work VPN.conf", None, "WireGuard"), "Work VPN");
        assert_eq!(profile_name("/tmp/x.ovpn", Some("y"), "OpenVPN"), "x");
        assert_eq!(profile_name("", Some("Home"), "WireGuard"), "Home");
        assert_eq!(profile_name("\u{7}.conf", None, "WireGuard"), "WireGuard");
        assert_eq!(profile_name(&"n".repeat(100), None, "W").len(), 64);
    }

    #[test]
    fn ipnet() {
        let n = IpNet::parse("10.77.0.0/24").unwrap();
        assert!(n.contains("10.77.0.10".parse().unwrap()));
        assert!(!n.contains("10.78.0.1".parse().unwrap()));
        assert!(IpNet::parse("0.0.0.0/0").unwrap().contains("8.8.8.8".parse().unwrap()));
        assert!(!IpNet::parse("0.0.0.0/0").unwrap().contains("::1".parse().unwrap()));
        assert!(IpNet::parse("::/0").unwrap().contains("2001:db8::1".parse().unwrap()));
        assert_eq!(IpNet::parse("1.2.3.4").unwrap().prefix, 32);
        assert!(IpNet::parse("1.2.3.4/33").is_none());
        assert!(IpNet::parse("1.2.3.4/").is_none());
        assert!(IpNet::parse("1.2.3.4/+1").is_none());
        assert!(IpNet::parse("x/1").is_none());
    }

    #[test]
    fn status_json() {
        let s = Status {
            state: State::Reconnecting,
            error: Some("x".into()),
            since_ms: 1,
            rx_bytes: 2,
            tx_bytes: 3,
            tunnel_ip: None,
            server: "a:1".into(),
        };
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j["state"], "reconnecting");
        assert_eq!(j["sinceMs"], 1);
        assert_eq!(j["tunnelIp"], serde_json::Value::Null);
        assert_eq!(serde_json::to_value(Kind::OpenVpn).unwrap(), "openvpn");
    }
}
