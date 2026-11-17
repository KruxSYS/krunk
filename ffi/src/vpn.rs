//! Krunk's in-browser VPN (the `vpn` crate) inside `libkrunk_ffi.so`, for Android.
//!
//! One [`KrunkVpn`] per app. It imports profiles and runs at most one VPN: a WireGuard (and, with
//! the `openvpn` feature, OpenVPN) tunnel in user space with a local proxy on
//! `127.0.0.1:<port>`. Android has no owner check for loopback peers, so the app hands in a
//! per-session random token: GeckoView and Krunk's own requests (`VpnSocks` in Kotlin) connect
//! with SOCKS5, username `krunk`, password = the token as 64 lowercase hex characters. Anything
//! else is closed without a reply. (The proxy also takes an HTTP head with
//! `Proxy-Authorization: Basic base64("krunk:" + hex)`; the app does not use it.) Scope ("private tabs only") and "reach the local network directly" are decided before
//! a request reaches the proxy (the engine's channel filter, the app's `VpnRoute`): the proxy
//! only ever sends through the tunnel, or refuses.
//!
//! Nothing here logs or returns profile text, keys, passwords or the token.

use krunk_vpn::{Credentials, ProxyAuth, State, Status, Vpn};
use std::sync::{Arc, Mutex, MutexGuard};
use zeroize::Zeroizing;

/// What an imported file is (no secrets in it).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VpnProfileInfo {
    /// `"wireguard"` or `"openvpn"`.
    pub kind: String,
    pub name: String,
    /// `host:port` of the server.
    pub server: String,
    /// An OpenVPN profile that asks for a username and password.
    pub needs_credentials: bool,
    /// Options Krunk ignores (scripts, DNS search domains, …), shown before saving.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum VpnState {
    Connecting,
    Connected,
    Reconnecting,
    /// Refused for good (password, certificate): no retry until started again.
    Failed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VpnStatus {
    pub state: VpnState,
    /// Human text for failed/reconnecting.
    pub error: Option<String>,
    /// When the current state began (unix ms).
    pub since_ms: i64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// Krunk's address inside the tunnel.
    pub tunnel_ip: Option<String>,
    /// `host:port` of the server.
    pub server: String,
}

/// Why an import or a start failed. The texts are for the user and never quote secrets.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum VpnError {
    /// Not a profile, broken, or too large.
    Import { reason: String },
    /// A profile kind or option this build does not support (OpenVPN without the feature).
    Unsupported { reason: String },
    /// An OpenVPN profile that needs a username and password, and none were given.
    NeedsCredentials,
    /// The proxy or the runtime could not be created.
    Start { reason: String },
    /// The token was not exactly 32 bytes.
    BadToken,
}

impl std::fmt::Display for VpnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VpnError::Import { reason } | VpnError::Unsupported { reason } | VpnError::Start { reason } => {
                f.write_str(reason)
            }
            VpnError::NeedsCredentials => f.write_str("This profile needs a username and password"),
            VpnError::BadToken => f.write_str("The VPN session token must be 32 bytes"),
        }
    }
}

impl std::error::Error for VpnError {}

/// Status changes, called on the VPN's own threads (at each state change and every ~2 s while
/// connected). Must return quickly and must not call [`KrunkVpn::stop`] from inside.
#[uniffi::export(with_foreign)]
pub trait VpnStatusListener: Send + Sync {
    fn on_status(&self, status: VpnStatus);
}

fn state(s: State) -> VpnState {
    match s {
        State::Connecting => VpnState::Connecting,
        State::Connected => VpnState::Connected,
        State::Reconnecting => VpnState::Reconnecting,
        State::Failed => VpnState::Failed,
        State::Stopped => VpnState::Stopped,
    }
}

fn status(s: &Status) -> VpnStatus {
    VpnStatus {
        state: state(s.state),
        error: s.error.clone(),
        since_ms: s.since_ms,
        rx_bytes: s.rx_bytes,
        tx_bytes: s.tx_bytes,
        tunnel_ip: s.tunnel_ip.clone(),
        server: s.server.clone(),
    }
}

fn import(file_name: &str, bytes: &[u8]) -> Result<(krunk_vpn::Profile, krunk_vpn::ProfileInfo), VpnError> {
    if bytes.len() > krunk_vpn::MAX_IMPORT_BYTES {
        return Err(VpnError::Import { reason: krunk_vpn::ImportError::TooLarge.to_string() });
    }
    // Zeroized after the import (the parsed profile keeps its own zeroizing copies).
    let text = Zeroizing::new(String::from_utf8_lossy(bytes).into_owned());
    krunk_vpn::import(file_name, &text).map_err(|e| match e {
        krunk_vpn::ImportError::Unsupported(m) => VpnError::Unsupported { reason: m },
        other => VpnError::Import { reason: other.to_string() },
    })
}

/// The app's VPN. Methods are cheap except [`start`](Self::start) and [`stop`](Self::stop),
/// which may take a few seconds (call them off the main thread).
#[derive(uniffi::Object)]
pub struct KrunkVpn {
    running: Mutex<Option<Vpn>>,
}

impl std::fmt::Debug for KrunkVpn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KrunkVpn")
    }
}

impl KrunkVpn {
    fn running(&self) -> MutexGuard<'_, Option<Vpn>> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[uniffi::export]
impl KrunkVpn {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(KrunkVpn { running: Mutex::new(None) })
    }

    /// Checks a file (at most 64 KiB) and says what it is; nothing is kept. The kind comes from
    /// the content, the file name only gives the profile its name.
    pub fn import_profile(&self, file_name: String, bytes: Vec<u8>) -> Result<VpnProfileInfo, VpnError> {
        let bytes = Zeroizing::new(bytes);
        let (_, info) = import(&file_name, &bytes)?;
        Ok(VpnProfileInfo {
            kind: info.kind.as_str().to_owned(),
            name: info.name,
            server: info.server,
            needs_credentials: info.needs_credentials,
            warnings: info.warnings,
        })
    }

    /// Starts the VPN with the stored file (stopping a running one first) and returns the proxy
    /// port. The tunnel connects in the background; `listener` hears every state. `token`: 32
    /// random bytes for this session.
    pub fn start(
        &self,
        file_name: String,
        bytes: Vec<u8>,
        username: Option<String>,
        password: Option<String>,
        token: Vec<u8>,
        listener: Arc<dyn VpnStatusListener>,
    ) -> Result<u16, VpnError> {
        let token = Zeroizing::new(token);
        let auth = ProxyAuth::from_slice(&token).ok_or(VpnError::BadToken)?;
        let bytes = Zeroizing::new(bytes);
        let (profile, _) = import(&file_name, &bytes)?;
        let creds = match (username, password) {
            (Some(u), Some(p)) => Some(Credentials { username: Zeroizing::new(u), password: Zeroizing::new(p) }),
            _ => None,
        };
        let mut running = self.running();
        if let Some(old) = running.take() {
            old.stop();
        }
        let vpn = Vpn::start_authenticated(profile, creds, auth, Box::new(move |s| listener.on_status(status(s))))
            .map_err(|e| match e {
                krunk_vpn::StartError::NeedsCredentials => VpnError::NeedsCredentials,
                other => VpnError::Start { reason: other.to_string() },
            })?;
        let port = vpn.proxy_addr().port();
        *running = Some(vpn);
        Ok(port)
    }

    /// Stops the tunnel and closes the proxy (it answers "not connected" until closed).
    pub fn stop(&self) {
        let vpn = self.running().take();
        if let Some(v) = vpn {
            v.stop();
        }
    }

    /// At exit: the tunnel stops in the background; the proxy keeps its port, refusing, until
    /// the process ends, so nothing else can take the port the engine still points at.
    pub fn stop_for_exit(&self) {
        if let Some(v) = self.running().take() {
            v.stop_for_exit();
        }
    }

    /// The running VPN's status; `None` when none runs.
    pub fn status(&self) -> Option<VpnStatus> {
        self.running().as_ref().map(|v| status(&v.status()))
    }

    /// The device's network changed (Wi-Fi ↔ mobile): reconnect now.
    pub fn network_changed(&self) {
        if let Some(v) = self.running().as_ref() {
            v.network_changed();
        }
    }

    /// True if `host` was refused within the last 30 s as outside the profile's routes or not
    /// found through the VPN (for "Not reachable through this VPN").
    pub fn refused_recently(&self, host: String) -> bool {
        self.running().as_ref().is_some_and(|v| v.refused_recently(&host))
    }

    /// `[accepted, refused]` credential checks of the running proxy in debug builds (device test
    /// T2); always empty in release builds.
    pub fn debug_auth_counts(&self) -> Vec<u64> {
        if !cfg!(debug_assertions) {
            return Vec::new();
        }
        self.running().as_ref().map_or_else(Vec::new, |v| {
            let (ok, refused) = v.auth_counts();
            vec![ok, refused]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::time::Duration;

    const WG: &str = "[Interface]\nPrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=\nAddress = 10.66.0.2/32\n\
                      [Peer]\nPublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\nEndpoint = 192.0.2.1:9\nAllowedIPs = 0.0.0.0/0\n";

    struct Seen(Mutex<Vec<VpnState>>);
    impl VpnStatusListener for Seen {
        fn on_status(&self, status: VpnStatus) {
            self.0.lock().unwrap().push(status.state);
        }
    }

    #[test]
    fn imports_wireguard_refuses_garbage() {
        let vpn = KrunkVpn::new();
        let info = vpn.import_profile("Home VPN.conf".into(), WG.as_bytes().to_vec()).unwrap();
        assert_eq!(
            (info.kind.as_str(), info.name.as_str(), info.server.as_str()),
            ("wireguard", "Home VPN", "192.0.2.1:9")
        );
        assert!(!info.needs_credentials);
        assert!(matches!(vpn.import_profile("x.txt".into(), b"hello".to_vec()), Err(VpnError::Import { .. })));
        let big = vec![b'#'; krunk_vpn::MAX_IMPORT_BYTES + 1];
        assert!(matches!(vpn.import_profile("big.conf".into(), big), Err(VpnError::Import { .. })));
    }

    #[test]
    fn openvpn_is_unsupported_without_the_feature() {
        // Feature unification (`cargo test -p vpn -p krunk_ffi`) can turn it on: ask the crate.
        if krunk_vpn::OPENVPN_SUPPORTED {
            return;
        }
        let vpn = KrunkVpn::new();
        let e = vpn.import_profile("b.ovpn".into(), b"client\nremote vpn.example 1194\n".to_vec()).unwrap_err();
        assert!(matches!(e, VpnError::Unsupported { .. }), "{e:?}");
    }

    #[test]
    fn start_needs_a_32_byte_token() {
        let vpn = KrunkVpn::new();
        let l = Arc::new(Seen(Mutex::new(Vec::new())));
        let e = vpn.start("a.conf".into(), WG.into(), None, None, vec![1; 31], l).unwrap_err();
        assert_eq!(e, VpnError::BadToken);
        assert!(vpn.status().is_none());
    }

    /// Start, the listener hears "connecting", the proxy wants the credential, stop.
    #[test]
    fn start_status_auth_stop() {
        let vpn = KrunkVpn::new();
        let seen = Arc::new(Seen(Mutex::new(Vec::new())));
        let token: Vec<u8> = (0u8..32).collect();
        let port = vpn.start("a.conf".into(), WG.into(), None, None, token.clone(), seen.clone()).unwrap();
        assert_ne!(port, 0);
        assert_eq!(vpn.status().unwrap().state, VpnState::Connecting);
        vpn.network_changed();
        assert!(!vpn.refused_recently("example.com".into()));
        // Without the credential: closed, nothing answered.
        let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(7))).unwrap();
        c.write_all(b"CONNECT example.com:443 HTTP/1.1\r\n\r\n").unwrap();
        let mut r = Vec::new();
        let _ = c.read_to_end(&mut r);
        assert!(r.is_empty());
        // SOCKS5 with it: the tunnel is not up, so "general failure", never a connection.
        let hex: String = token.iter().map(|b| format!("{b:02x}")).collect();
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(7))).unwrap();
        let mut m = vec![5, 1, 2, 1, 5];
        m.extend_from_slice(b"krunk");
        m.push(64);
        m.extend_from_slice(hex.as_bytes());
        m.extend_from_slice(&[5, 1, 0, 1, 10, 66, 0, 1, 0, 80]);
        s.write_all(&m).unwrap();
        let mut r = Vec::new();
        let _ = s.read_to_end(&mut r);
        assert_eq!(r, vec![5, 2, 1, 0, 5, 1, 0, 1, 0, 0, 0, 0, 0, 0]);
        let counts = vpn.debug_auth_counts();
        assert_eq!(counts, vec![1, 1], "debug build counts the checks");
        vpn.stop();
        assert!(vpn.status().is_none());
        assert!(std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(1)).is_err());
        let seen = seen.0.lock().unwrap();
        assert_eq!(seen.first(), Some(&VpnState::Connecting));
        assert_eq!(seen.last(), Some(&VpnState::Stopped));
    }
}
