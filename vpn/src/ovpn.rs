//! OpenVPN: profile inspection (the import rewrite) and the tunnel through OpenVPN 3 Core
//! (`openvpn-connect`, external TUN: Core hands us IP packets, no adapter).
//!
//! The import rewrite (spike risk R1: Core 3.11.7 silently ignores `remote-cert-tls`):
//! - `remote-cert-tls server` becomes `remote-cert-eku "TLS Web Server Authentication"`;
//! - without an EKU check the rewrite adds that line and a warning;
//! - `script-security`, `up`, `down`, `route-up` and other script/plugin/daemon options are
//!   dropped with a warning (Core never runs them anyway);
//! - an inline `<auth-user-pass>` pair is taken out of the text and kept as credentials;
//! - references to files (`ca ca.crt`) are refused: profiles must be self-contained.
//!
//! The rewrite is idempotent, so a stored rewritten text imports to the same profile.
//!
//! Failure detection (spike risk R3: `connect()` returns Ok even after `AUTH_FAILED`, and TLS
//! and certificate errors only show in Core's log while it keeps reconnecting): Core's events
//! and log lines are classified here. A refused password or certificate ends the session as
//! `failed` without a retry loop; timeouts end it for a retry with back-off.

use crate::service::{SessionEnd, Shared};
use crate::{Credentials, ImportError, IpNet};
use openvpn_connect::{
    Client, Config, Event, EventHandler, ExternalTun, ExternalTunConfig, ExternalTunInfo, ExternalTunIo,
    ExternalTunStartConfig,
};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use zeroize::{Zeroize, Zeroizing};

/// The EKU line the rewrite uses.
pub const SERVER_EKU: &str = "remote-cert-eku \"TLS Web Server Authentication\"";

/// An inspected `.ovpn` profile. `Debug` never shows the text or the credentials.
pub struct Profile {
    text: Zeroizing<String>,
    server: String,
    needs_auth: bool,
    inline: Option<Credentials>,
    client_cert: bool,
    routes: Vec<IpNet>,
}

impl Profile {
    /// The profile text after the rewrite. It holds keys: never log it.
    pub fn text(&self) -> &str {
        &self.text
    }
    /// `host:port` of the first remote.
    pub fn server(&self) -> &str {
        &self.server
    }
    /// `auth-user-pass` without an inline pair.
    pub fn needs_credentials(&self) -> bool {
        self.needs_auth && self.inline.is_none()
    }
    /// The profile asks for a username and password at all.
    pub fn uses_credentials(&self) -> bool {
        self.needs_auth
    }
    /// An inline `<auth-user-pass>` pair, if the file had one.
    pub fn inline_credentials(&self) -> Option<&Credentials> {
        self.inline.as_ref()
    }
}

impl fmt::Debug for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ovpn::Profile")
            .field("server", &self.server)
            .field("needs_auth", &self.needs_auth)
            .field("inline_credentials", &self.inline.is_some())
            .field("client_cert", &self.client_cert)
            .field("text", &format_args!("<{} bytes>", self.text.len()))
            .finish()
    }
}

/// The result of [`inspect`].
#[derive(Debug)]
pub struct Inspected {
    pub profile: Profile,
    pub warnings: Vec<String>,
    /// A name from the profile (`# Name = …`, or Core's friendly/profile name).
    pub name: Option<String>,
}

/// Options that run programs or touch the system; dropped with one warning.
const SCRIPTS: &[&str] = &[
    "script-security",
    "up",
    "down",
    "route-up",
    "route-pre-down",
    "ipchange",
    "plugin",
    "tls-verify",
    "learn-address",
    "client-connect",
    "client-disconnect",
    "auth-user-pass-verify",
    "up-restart",
    "up-delay",
    "down-pre",
];
/// Options for a daemon process; dropped with a warning.
const IGNORED: &[&str] = &[
    "management",
    "management-hold",
    "management-query-passwords",
    "management-client",
    "log",
    "log-append",
    "status",
    "writepid",
    "daemon",
    "chroot",
    "user",
    "group",
    "cd",
    "iproute",
    "syslog",
    "service",
    "auth-nocache",
    "pull-filter",
    "route-nopull",
    "register-dns",
    "block-outside-dns",
];
/// Options whose argument is a file.
const FILE_OPTIONS: &[&str] = &[
    "ca",
    "cert",
    "key",
    "tls-auth",
    "tls-crypt",
    "tls-crypt-v2",
    "pkcs12",
    "crl-verify",
    "extra-certs",
    "secret",
    "dh",
    "askpass",
    "http-proxy-user-pass",
    "config",
];

fn invalid(m: impl Into<String>) -> ImportError {
    ImportError::Invalid(m.into())
}

/// Printable, at most `n` chars (for texts from Core or the server).
fn clean(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(n).collect::<String>().trim().to_owned()
}

/// Rewrites and validates a profile. Core's own parser checks the result.
pub fn inspect(text: &str) -> Result<Inspected, ImportError> {
    // Sized once, so growing never leaves copies of the keys in freed memory.
    let mut out = String::with_capacity(text.len() + 256);
    let mut warnings: Vec<String> = Vec::new();
    let mut warn = |w: String| {
        if !warnings.contains(&w) && warnings.len() < 32 {
            warnings.push(w);
        }
    };
    let mut name = None;
    let mut has_eku = false;
    let mut needs_auth = false;
    let mut auth_directive = false;
    let mut inline: Option<Credentials> = None;
    let mut client_cert = false;
    let mut routes = Vec::new();
    let mut block: Option<String> = None;
    let mut auth_block: Option<Zeroizing<String>> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(tag) = &block {
            if line.eq_ignore_ascii_case(&format!("</{tag}>")) {
                if tag == "auth-user-pass" {
                    let pair = auth_block.take().unwrap_or_default();
                    let mut it = pair.lines().map(str::trim).filter(|l| !l.is_empty());
                    match (it.next(), it.next()) {
                        (Some(u), Some(p)) => {
                            inline =
                                Some(Credentials { username: u.to_owned(), password: Zeroizing::new(p.to_owned()) })
                        }
                        _ => return Err(invalid("The inline <auth-user-pass> needs a username and a password line")),
                    }
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
                block = None;
            } else if tag == "auth-user-pass" {
                let b = auth_block.get_or_insert_with(Default::default);
                b.push_str(raw);
                b.push('\n');
            } else {
                out.push_str(raw.trim_end());
                out.push('\n');
            }
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix('#').or_else(|| line.strip_prefix(';')) {
            if let Some((k, v)) = comment.split_once(['=', ':']) {
                if k.trim().eq_ignore_ascii_case("name") && name.is_none() && !v.trim().is_empty() {
                    name = Some(clean(v, 64));
                }
            }
            continue;
        }
        if let Some(tag) = line.strip_prefix('<').and_then(|l| l.strip_suffix('>')) {
            if let Some(closing) = tag.strip_prefix('/') {
                return Err(invalid(format!("A stray </{}>", clean(closing, 32))));
            }
            let tag = tag.to_ascii_lowercase();
            if !tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return Err(invalid("A broken inline block"));
            }
            match tag.as_str() {
                "auth-user-pass" => {
                    needs_auth = true;
                }
                "cert" | "pkcs12" => {
                    client_cert = true;
                    out.push_str(line);
                    out.push('\n');
                }
                _ => {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            block = Some(tag);
            continue;
        }
        let mut words = line.split_ascii_whitespace();
        let opt = words.next().unwrap_or("").to_ascii_lowercase();
        let opt = opt.strip_prefix("--").unwrap_or(&opt).to_owned();
        let args: Vec<&str> = words.collect();
        match opt.as_str() {
            "remote-cert-tls" => {
                if args.first().is_some_and(|a| a.eq_ignore_ascii_case("server")) {
                    has_eku = true;
                    out.push_str(SERVER_EKU);
                    out.push('\n');
                } else {
                    return Err(invalid("remote-cert-tls must be \"server\" in a client profile"));
                }
                continue;
            }
            "remote-cert-eku" => has_eku = true,
            o if SCRIPTS.contains(&o) => {
                warn("Scripts and plugins (up, down, script-security …) are ignored".into());
                continue;
            }
            o if IGNORED.contains(&o) => {
                warn(format!("{o} is ignored"));
                continue;
            }
            o if FILE_OPTIONS.contains(&o) => {
                if args.first().is_some_and(|a| a.eq_ignore_ascii_case("[inline]")) {
                    if o == "cert" || o == "pkcs12" {
                        client_cert = true;
                    }
                } else if o == "config" {
                    return Err(ImportError::Unsupported("The profile includes other files (config)".into()));
                } else {
                    return Err(ImportError::Unsupported(format!(
                        "The profile refers to the file \"{}\" ({o}); import a profile with the certificates and keys inline",
                        clean(args.first().copied().unwrap_or(""), 64)
                    )));
                }
            }
            "auth-user-pass" => {
                needs_auth = true;
                auth_directive = true;
                if !args.is_empty() {
                    warn(
                        "The password file of auth-user-pass is not read; enter the username and password in Krunk"
                            .into(),
                    );
                    out.push_str("auth-user-pass\n");
                    continue;
                }
            }
            "dev" | "dev-type" => {
                if args.first().is_some_and(|a| a.to_ascii_lowercase().starts_with("tap")) {
                    return Err(ImportError::Unsupported("TAP (layer 2) profiles are not supported".into()));
                }
            }
            "static-challenge" => {
                warn("Two-factor prompts (static-challenge) are not supported".into());
                continue;
            }
            "route" => {
                if let Some(net) = route_v4(&args) {
                    routes.push(net);
                }
            }
            "route-ipv6" => {
                if let Some(net) = args.first().and_then(|a| IpNet::parse(a)) {
                    routes.push(net);
                }
            }
            "redirect-gateway" => routes.extend(redirect(&args)),
            _ => {}
        }
        out.push_str(line);
        out.push('\n');
    }
    if block.is_some() {
        return Err(invalid("An inline block is not closed"));
    }
    if inline.is_some() && !auth_directive {
        out.push_str("auth-user-pass\n");
    }
    if !has_eku {
        out.push_str(SERVER_EKU);
        out.push('\n');
        warn("The profile did not check the server certificate's purpose; Krunk added the check".into());
    }

    let mut cfg = Config::new(out.as_str());
    cfg.disable_client_cert = !client_cert;
    let evaluated = openvpn_connect::evaluate_config(&cfg);
    cfg.content.zeroize();
    let ev = evaluated.map_err(|e| invalid(format!("OpenVPN rejected the profile: {}", clean(&e.to_string(), 200))))?;
    if ev.private_key_password_required {
        return Err(ImportError::Unsupported("Password-protected private keys are not supported".into()));
    }
    if ev.remote_host.is_empty() {
        return Err(invalid("The profile has no remote server"));
    }
    let host = clean(&ev.remote_host, 253);
    let port = if ev.remote_port.is_empty() { "1194".to_owned() } else { clean(&ev.remote_port, 5) };
    let server = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
    if name.is_none() {
        let n = if ev.friendly_name.is_empty() { &ev.profile_name } else { &ev.friendly_name };
        if !n.is_empty() {
            name = Some(clean(n, 64));
        }
    }
    Ok(Inspected {
        profile: Profile { text: Zeroizing::new(out), server, needs_auth, inline, client_cert, routes },
        warnings,
        name,
    })
}

/// `255.255.255.0` → 24 (contiguous masks only).
fn mask_prefix(mask: &str) -> Option<u8> {
    let m = u32::from(mask.parse::<Ipv4Addr>().ok()?);
    let ones = m.leading_ones();
    (m.checked_shl(ones).unwrap_or(0) == 0).then_some(ones as u8)
}

/// `route NET [MASK] [GW] [METRIC]` (literal addresses only).
fn route_v4(args: &[&str]) -> Option<IpNet> {
    let net: Ipv4Addr = args.first()?.parse().ok()?;
    let prefix = match args.get(1) {
        Some(m) if *m != "default" => mask_prefix(m)?,
        _ => 32,
    };
    IpNet::new(IpAddr::V4(net), prefix)
}

/// `redirect-gateway [def1] [ipv6] [!ipv4] …`: everything through the tunnel.
fn redirect(args: &[&str]) -> Vec<IpNet> {
    let mut r = Vec::new();
    if !args.iter().any(|a| a.eq_ignore_ascii_case("!ipv4")) {
        r.push(IpNet { addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED), prefix: 0 });
    }
    if args.iter().any(|a| a.eq_ignore_ascii_case("ipv6")) {
        r.push(IpNet { addr: IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), prefix: 0 });
    }
    r
}

/// What Core pushed (the external TUN's start options, one per line).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Pushed {
    pub addrs: Vec<IpNet>,
    pub routes: Vec<IpNet>,
    pub dns: Vec<IpAddr>,
    pub mtu: Option<usize>,
}

/// Parses the pushed options.
pub fn parse_pushed(options: &str) -> Pushed {
    let mut p = Pushed::default();
    for line in options.lines() {
        let w: Vec<&str> = line.split_ascii_whitespace().collect();
        match w.as_slice() {
            ["ifconfig", ip, second, ..] => {
                let Ok(ip) = ip.parse::<Ipv4Addr>() else { continue };
                p.addrs.push(IpNet { addr: IpAddr::V4(ip), prefix: 32 });
                match mask_prefix(second) {
                    Some(prefix) if second.starts_with("255.") => {
                        p.routes.extend(IpNet::new(IpAddr::V4(ip), prefix));
                    }
                    _ => {
                        if let Ok(peer) = second.parse::<Ipv4Addr>() {
                            p.routes.push(IpNet { addr: IpAddr::V4(peer), prefix: 32 });
                        }
                    }
                }
            }
            ["ifconfig-ipv6", net, ..] => {
                if let Some(n) = IpNet::parse(net) {
                    p.addrs.push(IpNet { addr: n.addr, prefix: 128 });
                    p.routes.push(n);
                }
            }
            ["route", rest @ ..] => p.routes.extend(route_v4(rest)),
            ["route-ipv6", net, ..] => p.routes.extend(IpNet::parse(net)),
            ["redirect-gateway", rest @ ..] => p.routes.extend(redirect(rest)),
            ["dhcp-option", kind, ip, ..] if kind.eq_ignore_ascii_case("dns") || kind.eq_ignore_ascii_case("dns6") => {
                if let Ok(ip) = ip.parse::<IpAddr>() {
                    if !p.dns.contains(&ip) {
                        p.dns.push(ip);
                    }
                }
            }
            ["dns", "server", _, "address", addrs @ ..] => {
                for a in addrs {
                    let a = a.strip_prefix('[').map_or(*a, |r| r.split(']').next().unwrap_or(r));
                    let ip = a.parse::<IpAddr>().ok().or_else(|| a.rsplit_once(':').and_then(|(h, _)| h.parse().ok()));
                    if let Some(ip) = ip {
                        if !p.dns.contains(&ip) {
                            p.dns.push(ip);
                        }
                    }
                }
            }
            ["tun-mtu", m, ..] => p.mtu = m.parse().ok().filter(|m| (576..=9000).contains(m)),
            _ => {}
        }
    }
    p
}

// ---------------------------------------------------------------------------------------------
// The tunnel.

/// A failure that ends the session without a retry.
pub const AUTH_FAILED: &str = "The username or password was refused";
pub const CERT_FAILED: &str = "The server's certificate was not accepted";

enum Msg {
    Event(Event),
    /// A log line matched a permanent failure.
    Failure(&'static str),
    Ended,
}

/// Classifies a Core log line: certificate and TLS failures that Core itself only retries.
/// Only the classification leaves this function; the line is never stored or logged.
pub fn classify_log(line: &str) -> Option<&'static str> {
    // The rustls backend's verifier messages (openvpn-connect-sys rust_backend/tls.rs) and
    // Core's own error name.
    const CERT: &[&str] = &[
        "peer certificate verification failed",
        "peer certificate fingerprint does not match",
        "peer certificate has",
        "peer certificate key usage",
        "peer certificate extended key usage",
        "did not match the peer certificate",
        "invalid peer certificate",
        "cert_verify_fail",
    ];
    let l = line.to_ascii_lowercase();
    if CERT.iter().any(|p| l.contains(p)) {
        return Some(CERT_FAILED);
    }
    if l.contains("auth_failed") {
        return Some(AUTH_FAILED);
    }
    None
}

struct Tun {
    shared: Arc<Shared>,
    io: Mutex<Option<ExternalTunIo>>,
    ip: Mutex<String>,
    mtu: AtomicUsize,
    base_routes: Vec<IpNet>,
}

struct Sink(ExternalTunIo);

impl crate::net::PacketSink for Sink {
    fn send(&self, packet: &[u8]) {
        let _ = self.0.receive(packet);
    }
}

impl ExternalTun for Tun {
    fn configure(&self, config: &ExternalTunConfig) -> bool {
        if let Ok(m) = usize::try_from(config.mtu) {
            if (576..=9000).contains(&m) {
                self.mtu.store(m, Ordering::Relaxed);
            }
        }
        true
    }

    fn start(&self, config: &ExternalTunStartConfig, io: ExternalTunIo) {
        let pushed = parse_pushed(&config.options);
        let mtu = pushed.mtu.unwrap_or_else(|| self.mtu.load(Ordering::Relaxed));
        self.shared.net.configure(pushed.addrs.clone(), mtu);
        let mut routes = pushed.routes;
        routes.extend(self.base_routes.iter().copied());
        self.shared.set_routes(routes);
        self.shared.dns.set_servers(pushed.dns);
        *self.ip.lock().unwrap_or_else(PoisonError::into_inner) = pushed
            .addrs
            .iter()
            .find(|a| a.addr.is_ipv4())
            .or(pushed.addrs.first())
            .map(|a| a.addr.to_string())
            .unwrap_or_default();
        self.shared.net.set_sink(Some(Arc::new(Sink(io.clone()))));
        let _ = io.pre_tun_config();
        let _ = io.pre_route_config();
        let _ = io.connected();
        *self.io.lock().unwrap_or_else(PoisonError::into_inner) = Some(io);
    }

    fn stop(&self) {
        self.shared.net.set_sink(None);
        self.io.lock().unwrap_or_else(PoisonError::into_inner).take();
    }

    fn send(&self, packet: &[u8]) -> bool {
        self.shared.net.inject(packet);
        true
    }

    fn info(&self) -> ExternalTunInfo {
        ExternalTunInfo {
            name: "krunk".into(),
            vpn_ipv4: self.ip.lock().unwrap_or_else(PoisonError::into_inner).clone(),
            mtu: i32::try_from(self.mtu.load(Ordering::Relaxed)).unwrap_or(1500),
            ..Default::default()
        }
    }
}

struct Handler {
    tun: Arc<Tun>,
    tx: mpsc::UnboundedSender<Msg>,
}

impl EventHandler for Handler {
    fn event(&self, event: Event) {
        let _ = self.tx.send(Msg::Event(event));
    }

    fn log(&self, text: &str) {
        if let Some(f) = classify_log(text) {
            let _ = self.tx.send(Msg::Failure(f));
        }
    }

    fn external_tun(&self) -> Option<&dyn ExternalTun> {
        Some(self.tun.as_ref())
    }
}

/// What an event means for the session.
enum Meaning {
    Connected,
    Lost(&'static str),
    Fail(String),
    Note(&'static str),
    Nothing,
}

fn meaning(e: &Event) -> Meaning {
    match e.name.as_str() {
        "CONNECTED" => Meaning::Connected,
        "RECONNECTING" => Meaning::Lost("The connection was lost; reconnecting"),
        "AUTH_FAILED" => Meaning::Fail(AUTH_FAILED.into()),
        "CERT_VERIFY_FAIL" => Meaning::Fail(CERT_FAILED.into()),
        "TLS_VERSION_MIN" => Meaning::Fail("The server's TLS version is too old".into()),
        "NEED_CREDS" => Meaning::Fail("This profile needs a username and password".into()),
        "DYNAMIC_CHALLENGE" => {
            Meaning::Fail("The server asks for a one-time code, which Krunk does not support".into())
        }
        "EPKI_ERROR" | "EPKI_INVALID_ALIAS" => Meaning::Fail("The profile needs a client certificate and key".into()),
        "CLIENT_HALT" => {
            if e.info.to_ascii_uppercase().contains("AUTH") {
                Meaning::Fail(AUTH_FAILED.into())
            } else {
                let why = clean(&e.info, 120);
                Meaning::Fail(if why.is_empty() {
                    "The server ended the session".into()
                } else {
                    format!("The server ended the session: {why}")
                })
            }
        }
        "TUN_SETUP_FAILED" | "TUN_IFACE_CREATE" | "TUN_IFACE_DISABLED" | "TUN_ERROR" => {
            Meaning::Fail("OpenVPN could not set up the tunnel".into())
        }
        "CONNECTION_TIMEOUT" => Meaning::Note("The server did not answer (timeout)"),
        "INACTIVE_TIMEOUT" => Meaning::Note("The connection was idle too long"),
        "TRANSPORT_ERROR" => Meaning::Note("A network error"),
        "PROXY_ERROR" | "PROXY_NEED_CREDS" => Meaning::Note("The profile's proxy failed"),
        "KEEPALIVE_TIMEOUT" => Meaning::Lost("The server stopped answering; reconnecting"),
        _ if e.error => Meaning::Note("The connection failed"),
        _ => Meaning::Nothing,
    }
}

/// One OpenVPN session: Core reconnects by itself within it; it ends on a permanent failure,
/// Core's connection timeout, or `stop`.
pub(crate) async fn session(
    p: &Profile,
    creds: Option<&Credentials>,
    shared: &Arc<Shared>,
    stop: &mut watch::Receiver<bool>,
) -> SessionEnd {
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let tun = Arc::new(Tun {
        shared: shared.clone(),
        io: Mutex::new(None),
        ip: Mutex::new(String::new()),
        mtu: AtomicUsize::new(1500),
        base_routes: p.routes.clone(),
    });
    let client = match Client::new(Handler { tun: tun.clone(), tx: tx.clone() }) {
        Ok(c) => c,
        Err(e) => return SessionEnd::Retry(format!("OpenVPN could not start: {}", clean(&e.to_string(), 120))),
    };
    let mut cfg = Config::new(p.text.as_str());
    cfg.disable_client_cert = !p.client_cert;
    cfg.connection_timeout_seconds = 30;
    cfg.compression_mode = "no".into();
    cfg.gui_version = format!("Krunk {}", env!("CARGO_PKG_VERSION"));
    cfg.retry_on_auth_failed = false;
    let evaluated = client.evaluate(&cfg);
    cfg.content.zeroize();
    if let Err(e) = evaluated {
        return SessionEnd::Failed(format!("OpenVPN rejected the profile: {}", clean(&e.to_string(), 160)));
    }
    if p.needs_auth {
        let Some(c) = creds else { return SessionEnd::Failed("This profile needs a username and password".into()) };
        let mut oc = openvpn_connect::Credentials::new(c.username.as_str(), c.password.as_str());
        let r = client.provide_credentials(&oc);
        oc.password.zeroize();
        oc.username.zeroize();
        if r.is_err() {
            return SessionEnd::Failed("The username or password could not be used".into());
        }
    }

    let running = client.clone();
    let done_tx = tx.clone();
    let spawned = std::thread::Builder::new().name("krunk-vpn-ovpn".into()).spawn(move || {
        let _ = running.connect();
        drop(running);
        let _ = done_tx.send(Msg::Ended);
    });
    if spawned.is_err() {
        return SessionEnd::Retry("OpenVPN could not start".into());
    }
    drop(tx);

    let mut failure: Option<String> = None;
    let mut note: Option<&'static str> = None;
    let mut connected = false;
    loop {
        let msg = tokio::select! {
            _ = crate::service::stopped(stop) => {
                client.stop();
                // Wait (bounded) for connect() to return so the session is gone.
                let _ = tokio::time::timeout(Duration::from_secs(5), async {
                    while let Some(m) = rx.recv().await {
                        if matches!(m, Msg::Ended) { break; }
                    }
                }).await;
                return SessionEnd::Stopped;
            }
            m = rx.recv() => m,
        };
        let Some(msg) = msg else { return SessionEnd::Retry("OpenVPN stopped".into()) };
        match msg {
            Msg::Ended => {
                return match failure {
                    Some(f) => SessionEnd::Failed(f),
                    None => SessionEnd::Retry(note.unwrap_or("The connection ended").to_owned()),
                };
            }
            Msg::Failure(f) => {
                if failure.is_none() {
                    failure = Some(f.to_owned());
                    client.stop();
                }
            }
            Msg::Event(e) => match meaning(&e) {
                Meaning::Connected => {
                    connected = true;
                    note = None;
                    let ip = tun.ip.lock().unwrap_or_else(PoisonError::into_inner).clone();
                    shared.connected((!ip.is_empty()).then_some(ip));
                }
                Meaning::Lost(why) => {
                    if connected {
                        shared.set_state(crate::State::Reconnecting, Some(why.to_owned()));
                    }
                    connected = false;
                }
                Meaning::Fail(why) => {
                    if failure.is_none() {
                        failure = Some(why);
                        client.stop();
                    }
                }
                Meaning::Note(why) => note = Some(why),
                Meaning::Nothing => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throw-away CA (no key kept), from the spike: Core parses it during evaluation.
    const CA: &str = "-----BEGIN CERTIFICATE-----
MIIBejCCASGgAwIBAgIUAecf+zsereYcFyJAY5Kw5GoN0P0wCgYIKoZIzj0EAwIw
EzERMA8GA1UEAwwIcHJvYmUtY2EwHhcNMjYxMTEwMTYzNzE2WhcNMzYxMTA3MTYz
NzE2WjATMREwDwYDVQQDDAhwcm9iZS1jYTBZMBMGByqGSM49AgEGCCqGSM49AwEH
A0IABKIOoJUQsmektJOukRpVDF7kS4JMquy2xTTwfTwkim11L4NWUcwRzJGYQPbm
PXaw3vHFblpKk2S8JXw/KwwoQMWjUzBRMB0GA1UdDgQWBBREjN7pYAtw7RsOPZs5
NrMrxYdt6TAfBgNVHSMEGDAWgBREjN7pYAtw7RsOPZs5NrMrxYdt6TAPBgNVHRMB
Af8EBTADAQH/MAoGCCqGSM49BAMCA0cAMEQCIA8+fcJ5s1xvIORlZwEV628VD+qH
WsGQyQBatI0mNsWAAiARGFrD/m8QX5DVtKf+jThJW0arJCQwnbBPWhJctLrNFw==
-----END CERTIFICATE-----";

    fn profile(extra: &str) -> String {
        format!(
            "client\ndev tun\nproto udp\nremote vpn.example.test 1194\nnobind\nauth-user-pass\n{extra}\n<ca>\n{CA}\n</ca>\n"
        )
    }

    #[test]
    fn rewrites_remote_cert_tls() {
        let t = profile("remote-cert-tls server\nscript-security 2\nup /etc/openvpn/up.sh\ndown x\nroute-up y\nverb 3");
        let i = inspect(&t).unwrap();
        let text = i.profile.text();
        assert!(text.contains(SERVER_EKU), "{text}");
        assert!(!text.contains("remote-cert-tls"));
        assert!(!text.contains("script-security") && !text.contains("up.sh") && !text.contains("route-up"));
        assert_eq!(i.warnings.len(), 1, "{:?}", i.warnings);
        assert!(i.profile.needs_credentials());
        assert_eq!(i.profile.server(), "vpn.example.test:1194");
        // Idempotent: the rewritten text imports to the same text, without new warnings.
        let again = inspect(text).unwrap();
        assert_eq!(again.profile.text(), text);
        assert!(again.warnings.is_empty(), "{:?}", again.warnings);
    }

    #[test]
    fn adds_eku_when_missing() {
        let i = inspect(&profile("")).unwrap();
        assert_eq!(i.profile.text().matches(SERVER_EKU).count(), 1);
        assert!(i.warnings.iter().any(|w| w.contains("added the check")), "{:?}", i.warnings);
        let i = inspect(&profile("remote-cert-eku \"TLS Web Server Authentication\"")).unwrap();
        assert_eq!(i.profile.text().matches("remote-cert-eku").count(), 1);
        assert!(i.warnings.is_empty());
    }

    #[test]
    fn inline_credentials_leave_the_text() {
        let t = profile("<auth-user-pass>\nkrunk\nsecret-pw\n</auth-user-pass>");
        let i = inspect(&t).unwrap();
        assert!(!i.profile.text().contains("secret-pw"));
        assert!(!i.profile.needs_credentials());
        assert!(i.profile.uses_credentials());
        let c = i.profile.inline_credentials().unwrap();
        assert_eq!((c.username.as_str(), c.password.as_str()), ("krunk", "secret-pw"));
        assert!(!format!("{:?}", i.profile).contains("secret-pw"));
        let file = profile("").replace("auth-user-pass\n", "auth-user-pass /etc/pass.txt\n");
        let i = inspect(&file).unwrap();
        assert!(i.profile.needs_credentials());
        assert!(i.profile.text().contains("\nauth-user-pass\n"));
        assert!(i.warnings.iter().any(|w| w.contains("password file")));
    }

    #[test]
    fn refuses_bad_profiles() {
        for (t, needle) in [
            (profile("ca /etc/ca.crt"), "/etc/ca.crt"),
            (profile("tls-auth ta.key 1"), "ta.key"),
            (profile("config other.ovpn"), "other files"),
            (profile("remote-cert-tls client"), "must be"),
            (profile("").replace("dev tun", "dev tap0"), "TAP"),
            (profile("<cert>\nnot closed"), "not closed"),
            (profile("</key>"), "stray"),
            ("client\nremote\n<ca>\nnot a cert\n</ca>\n".to_owned(), "OpenVPN rejected"),
        ] {
            let e = inspect(&t).unwrap_err().to_string();
            assert!(e.contains(needle), "{needle}: {e}");
        }
    }

    #[test]
    fn profile_routes_and_name() {
        let t = format!("# Name: Work\n{}", profile("route 10.1.0.0 255.255.0.0\nredirect-gateway def1"));
        let i = inspect(&t).unwrap();
        assert_eq!(i.name.as_deref(), Some("Work"));
        assert_eq!(i.profile.routes, vec![IpNet::parse("10.1.0.0/16").unwrap(), IpNet::parse("0.0.0.0/0").unwrap()]);
    }

    #[test]
    fn pushed_options() {
        let p = parse_pushed(
            "ifconfig 10.88.0.2 255.255.255.0\nroute 10.77.0.0 255.255.255.0\nroute-gateway 10.88.0.1\n\
             dhcp-option DNS 10.77.0.53\ndhcp-option DOMAIN corp\ntun-mtu 1400\ndns server 0 address 10.77.0.54 [fd00::53]:53\n\
             ifconfig-ipv6 fd00::2/64 fd00::1\nredirect-gateway def1 ipv6\nroute 1.2.3.4 255.0.255.0\n",
        );
        assert_eq!(p.addrs, vec![IpNet::parse("10.88.0.2/32").unwrap(), IpNet::parse("fd00::2/128").unwrap()]);
        assert!(p.routes.contains(&IpNet::parse("10.88.0.2/24").unwrap()));
        assert!(p.routes.contains(&IpNet::parse("10.77.0.0/24").unwrap()));
        assert!(p.routes.contains(&IpNet::parse("0.0.0.0/0").unwrap()));
        assert!(p.routes.contains(&IpNet::parse("::/0").unwrap()));
        assert!(!p.routes.iter().any(|r| r.addr.to_string() == "1.2.3.4"));
        assert_eq!(p.dns.len(), 3);
        assert_eq!(p.mtu, Some(1400));
        // net30 / p2p: the second address is the peer.
        let p = parse_pushed("ifconfig 10.8.0.6 10.8.0.5\n");
        assert_eq!(p.routes, vec![IpNet::parse("10.8.0.5/32").unwrap()]);
    }

    #[test]
    fn masks() {
        assert_eq!(mask_prefix("255.255.255.0"), Some(24));
        assert_eq!(mask_prefix("0.0.0.0"), Some(0));
        assert_eq!(mask_prefix("255.255.255.255"), Some(32));
        assert_eq!(mask_prefix("255.0.255.0"), None);
    }

    #[test]
    fn log_classifier() {
        assert_eq!(
            classify_log(
                "Client exception in transport_recv: Rust TLS write ciphertext failed: unexpected error: peer certificate verification failed: UnknownIssuer"
            ),
            Some(CERT_FAILED)
        );
        assert_eq!(classify_log("verify-x509-name did not match the peer certificate"), Some(CERT_FAILED));
        assert_eq!(
            classify_log("peer certificate extended key usage does not include TLS Web Server Authentication"),
            Some(CERT_FAILED)
        );
        assert_eq!(
            classify_log("Tunnel Options:V4,dev-type tun,link-mtu 1543,tun-mtu 1500,proto TCPv4_CLIENT,tls-client"),
            None
        );
        assert_eq!(classify_log("Transport Error: connection refused"), None);
        assert_eq!(classify_log("EVENT: AUTH_FAILED"), Some(AUTH_FAILED));
        assert_eq!(classify_log("Session is ACTIVE"), None);
    }
}
