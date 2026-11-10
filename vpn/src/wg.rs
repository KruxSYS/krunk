//! WireGuard: the `.conf` parser and the tunnel (boringtun's `noise::Tunn` over a tokio UDP
//! socket, with its timers, keepalives and handshake retries).
//!
//! Only `[Interface]` and one `[Peer]` are read. `PreUp`/`PostUp`/`PreDown`/`PostDown`,
//! `Table`, `FwMark`, `SaveConfig`, `ListenPort` and DNS search domains are reported as ignored
//! and never run. The private and preshared keys are zeroized when the config is dropped.

use crate::service::{SessionEnd, Shared};
use crate::{ImportError, IpNet};
use base64ct::{Base64, Encoding};
use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use zeroize::Zeroizing;

/// The most AllowedIPs, Address and DNS entries a config may have.
const MAX_LIST: usize = 1024;
/// WireGuard's usual MTU (1500 minus the IPv6 + UDP + WireGuard overhead).
pub const DEFAULT_MTU: u16 = 1420;

/// A parsed WireGuard config. `Debug` never shows the keys.
pub struct Config {
    private_key: Zeroizing<[u8; 32]>,
    peer_public: [u8; 32],
    preshared: Option<Zeroizing<[u8; 32]>>,
    addresses: Vec<IpNet>,
    dns: Vec<IpAddr>,
    mtu: u16,
    allowed_ips: Vec<IpNet>,
    endpoint: String,
    keepalive: Option<u16>,
}

impl Config {
    /// `host:port` of the peer.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    /// Our addresses inside the tunnel.
    pub fn addresses(&self) -> &[IpNet] {
        &self.addresses
    }
    pub fn dns(&self) -> &[IpAddr] {
        &self.dns
    }
    pub fn mtu(&self) -> u16 {
        self.mtu
    }
    pub fn allowed_ips(&self) -> &[IpNet] {
        &self.allowed_ips
    }
    pub fn persistent_keepalive(&self) -> Option<u16> {
        self.keepalive
    }
    /// The peer's public key (base64), e.g. for display.
    pub fn peer_public_key(&self) -> String {
        Base64::encode_string(&self.peer_public)
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("wg::Config")
            .field("private_key", &"<redacted>")
            .field("peer_public", &self.peer_public_key())
            .field("preshared", &self.preshared.as_ref().map(|_| "<redacted>"))
            .field("addresses", &self.addresses)
            .field("dns", &self.dns)
            .field("mtu", &self.mtu)
            .field("allowed_ips", &self.allowed_ips)
            .field("endpoint", &self.endpoint)
            .field("keepalive", &self.keepalive)
            .finish()
    }
}

/// The result of [`parse`].
#[derive(Debug)]
pub struct Parsed {
    pub config: Config,
    pub warnings: Vec<String>,
    /// A `# Name = …` comment, if the file has one.
    pub name: Option<String>,
}

fn invalid(m: impl Into<String>) -> ImportError {
    ImportError::Invalid(m.into())
}

/// Decodes a base64 WireGuard key (32 bytes). The decoded bytes are zeroized on error paths too.
fn key(value: &str, what: &str) -> Result<Zeroizing<[u8; 32]>, ImportError> {
    let mut out = Zeroizing::new([0u8; 32]);
    let mut buf = Zeroizing::new([0u8; 33]);
    match Base64::decode(value.trim(), &mut buf[..]) {
        Ok(b) if b.len() == 32 => {
            out.copy_from_slice(b);
            Ok(out)
        }
        _ => Err(invalid(format!("{what} is not a valid WireGuard key"))),
    }
}

/// Parses `host:port` or `[v6]:port` (port 1–65535).
pub(crate) fn parse_endpoint(s: &str) -> Option<(String, u16)> {
    let s = s.trim();
    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        let (h, p) = rest.split_once("]:")?;
        h.parse::<std::net::Ipv6Addr>().ok()?;
        (h, p)
    } else {
        let (h, p) = s.rsplit_once(':')?;
        if h.contains(':') {
            return None;
        }
        (h, p)
    };
    if host.is_empty() || host.len() > 253 || !p_ok(port) {
        return None;
    }
    if host.parse::<IpAddr>().is_err()
        && !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_')
    {
        return None;
    }
    let port: u16 = port.parse().ok()?;
    (port != 0).then(|| (host.to_owned(), port))
}

fn p_ok(p: &str) -> bool {
    !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit())
}

fn list(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|v| !v.is_empty())
}

/// Parses a WireGuard `.conf` text.
pub fn parse(text: &str) -> Result<Parsed, ImportError> {
    #[derive(PartialEq)]
    enum Section {
        None,
        Interface,
        Peer,
        Other,
    }
    let mut section = Section::None;
    let mut interfaces = 0;
    let mut peers = 0;
    let mut warnings: Vec<String> = Vec::new();
    let mut warn = |w: String| {
        if !warnings.contains(&w) && warnings.len() < 32 {
            warnings.push(w);
        }
    };
    let mut name = None;
    let mut private_key = None;
    let mut peer_public = None;
    let mut preshared = None;
    let mut addresses = Vec::new();
    let mut dns = Vec::new();
    let mut mtu = None;
    let mut allowed_ips = Vec::new();
    let mut endpoint = None;
    let mut keepalive = None;
    let mut seen: Vec<String> = Vec::new();

    for (n, raw) in text.lines().enumerate() {
        let line_no = n + 1;
        let (content, comment) = match raw.find('#') {
            Some(i) => (&raw[..i], Some(&raw[i + 1..])),
            None => (raw, None),
        };
        if let Some(c) = comment {
            let c = c.trim();
            if let Some((k, v)) = c.split_once(['=', ':']) {
                if k.trim().eq_ignore_ascii_case("name") && name.is_none() && !v.trim().is_empty() {
                    name = Some(v.trim().chars().take(64).collect());
                }
            }
        }
        let line = content.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) else {
                return Err(invalid(format!("Line {line_no}: a broken section header")));
            };
            seen.clear();
            section = match s.trim().to_ascii_lowercase().as_str() {
                "interface" => {
                    interfaces += 1;
                    Section::Interface
                }
                "peer" => {
                    peers += 1;
                    Section::Peer
                }
                _ => {
                    warn(format!("The section [{}] is ignored", s.trim().chars().take(32).collect::<String>()));
                    Section::Other
                }
            };
            if interfaces > 1 {
                return Err(invalid("The file has more than one [Interface] section"));
            }
            if peers > 1 {
                return Err(ImportError::Unsupported("Only one [Peer] is supported".into()));
            }
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(invalid(format!("Line {line_no}: expected \"Key = Value\"")));
        };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim();
        if section == Section::Other {
            continue;
        }
        if section == Section::None {
            return Err(invalid(format!("Line {line_no}: a setting outside [Interface] or [Peer]")));
        }
        let repeatable =
            matches!(k.as_str(), "address" | "dns" | "allowedips" | "preup" | "postup" | "predown" | "postdown");
        if !repeatable {
            if seen.contains(&k) {
                return Err(invalid(format!("Line {line_no}: {k} appears twice")));
            }
            seen.push(k.clone());
        }
        match (&section, k.as_str()) {
            (Section::Interface, "privatekey") => private_key = Some(key(v, "PrivateKey")?),
            (Section::Interface, "address") => {
                for a in list(v) {
                    let net = IpNet::parse(a).ok_or_else(|| invalid(format!("Line {line_no}: a bad Address")))?;
                    addresses.push(net);
                }
            }
            (Section::Interface, "dns") => {
                for d in list(v) {
                    match d.parse::<IpAddr>() {
                        Ok(ip) => dns.push(ip),
                        Err(_) => warn("DNS search domains are ignored".into()),
                    }
                }
            }
            (Section::Interface, "mtu") => {
                let m: u16 = v.parse().map_err(|_| invalid(format!("Line {line_no}: a bad MTU")))?;
                if !(576..=9000).contains(&m) {
                    return Err(invalid(format!("Line {line_no}: the MTU must be between 576 and 9000")));
                }
                mtu = Some(m);
            }
            (Section::Interface, "preup" | "postup" | "predown" | "postdown") => {
                warn("PreUp, PostUp, PreDown and PostDown commands are ignored (never run)".into());
            }
            (Section::Interface, "listenport" | "fwmark" | "table" | "saveconfig") => {
                warn(format!("{} is ignored", canonical(&k)));
            }
            (Section::Interface, "jc" | "jmin" | "jmax" | "s1" | "s2" | "h1" | "h2" | "h3" | "h4") => {
                return Err(ImportError::Unsupported("AmneziaWG configurations are not supported".into()));
            }
            (Section::Peer, "publickey") => peer_public = Some(*key(v, "PublicKey")?),
            (Section::Peer, "presharedkey") => preshared = Some(key(v, "PresharedKey")?),
            (Section::Peer, "allowedips") => {
                for a in list(v) {
                    let net =
                        IpNet::parse(a).ok_or_else(|| invalid(format!("Line {line_no}: a bad AllowedIPs entry")))?;
                    allowed_ips.push(net);
                }
            }
            (Section::Peer, "endpoint") => {
                let (h, p) = parse_endpoint(v).ok_or_else(|| invalid(format!("Line {line_no}: a bad Endpoint")))?;
                endpoint = Some(if h.contains(':') { format!("[{h}]:{p}") } else { format!("{h}:{p}") });
            }
            (Section::Peer, "persistentkeepalive") => {
                keepalive = if v.eq_ignore_ascii_case("off") {
                    None
                } else {
                    match v.parse::<u16>() {
                        Ok(0) => None,
                        Ok(s) => Some(s),
                        Err(_) => return Err(invalid(format!("Line {line_no}: a bad PersistentKeepalive"))),
                    }
                };
            }
            _ => warn(format!("Unknown setting {} is ignored", k.chars().take(32).collect::<String>())),
        }
        if addresses.len() > MAX_LIST || allowed_ips.len() > MAX_LIST || dns.len() > MAX_LIST {
            return Err(invalid("The file has too many entries"));
        }
    }

    if interfaces == 0 {
        return Err(invalid("The file has no [Interface] section"));
    }
    if peers == 0 {
        return Err(invalid("The file has no [Peer] section"));
    }
    let private_key = private_key.ok_or_else(|| invalid("[Interface] has no PrivateKey"))?;
    let peer_public = peer_public.ok_or_else(|| invalid("[Peer] has no PublicKey"))?;
    let endpoint = endpoint.ok_or_else(|| {
        ImportError::Unsupported("[Peer] has no Endpoint (Krunk connects to the server, it does not listen)".into())
    })?;
    if addresses.is_empty() {
        return Err(invalid("[Interface] has no Address"));
    }
    if allowed_ips.is_empty() {
        return Err(invalid("[Peer] has no AllowedIPs"));
    }
    if addresses.iter().filter(|a| a.addr.is_ipv4()).count() > 1
        || addresses.iter().filter(|a| a.addr.is_ipv6()).count() > 1
    {
        warn("Only the first IPv4 and the first IPv6 Address are used".into());
    }
    let v4 = addresses.iter().find(|a| a.addr.is_ipv4()).copied();
    let v6 = addresses.iter().find(|a| a.addr.is_ipv6()).copied();
    let addresses: Vec<IpNet> = v4.into_iter().chain(v6).collect();
    dns.truncate(4);
    Ok(Parsed {
        config: Config {
            private_key,
            peer_public,
            preshared,
            addresses,
            dns,
            mtu: mtu.unwrap_or(DEFAULT_MTU),
            allowed_ips,
            endpoint,
            keepalive,
        },
        warnings,
        name,
    })
}

fn canonical(k: &str) -> &'static str {
    match k {
        "listenport" => "ListenPort",
        "fwmark" => "FwMark",
        "table" => "Table",
        _ => "SaveConfig",
    }
}

// ---------------------------------------------------------------------------------------------
// The tunnel.

/// How long a first handshake may take before the session is given up.
const HANDSHAKE_GIVE_UP: Duration = Duration::from_secs(15);
/// Probe (a fresh handshake) after this much silence while idle …
const PROBE_IDLE: Duration = Duration::from_secs(30);
/// … or after this much silence since we sent data.
const PROBE_AFTER_SEND: Duration = Duration::from_secs(4);
/// An unanswered probe is repeated after this long; two unanswered probes end the session.
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);
const TICK: Duration = Duration::from_millis(250);
/// This many receive errors in a row end the session (it is retried with back-off).
const RECV_ERRORS_GIVE_UP: u32 = 200;

/// The pause after the `n`th receive error in a row: 10 ms, doubling, at most 1 s.
fn recv_backoff(n: u32) -> Duration {
    Duration::from_millis((10u64 << n.saturating_sub(1).min(7)).min(1000))
}

/// Hands smoltcp's packets to the WireGuard task.
struct Sink(mpsc::Sender<Vec<u8>>);

impl crate::net::PacketSink for Sink {
    fn send(&self, packet: &[u8]) {
        // A full queue drops the packet, like a congested link; TCP retransmits.
        let _ = self.0.try_send(packet.to_vec());
    }
}

/// One session: resolve, handshake, carry packets until the peer goes silent or `stop`.
pub(crate) async fn session(cfg: &Config, shared: &Arc<Shared>, stop: &mut watch::Receiver<bool>) -> SessionEnd {
    let endpoint = match resolve(&cfg.endpoint).await {
        Ok(ep) => ep,
        Err(e) => return SessionEnd::Retry(e),
    };
    let bind: SocketAddr = if endpoint.is_ipv4() { ([0, 0, 0, 0], 0).into() } else { ([0u16; 8], 0).into() };
    let sock = match UdpSocket::bind(bind).await {
        Ok(s) => s,
        Err(_) => return SessionEnd::Retry("No network".into()),
    };
    if sock.connect(endpoint).await.is_err() {
        return SessionEnd::Retry("The server is unreachable".into());
    }
    let index = u32::from_le_bytes(crate::random::<4>()) >> 8;
    // The keys are copied straight into boringtun (no copy of ours lives on in this session's
    // state; the config keeps the zeroizing originals).
    let mut tunn = Tunn::new(
        StaticSecret::from(*cfg.private_key),
        PublicKey::from(cfg.peer_public),
        cfg.preshared.as_deref().copied(),
        cfg.keepalive,
        index,
        None,
    );
    let mtu = usize::from(cfg.mtu);
    let mut out = vec![0u8; (mtu + 64).max(256)];
    let mut rbuf = vec![0u8; 65536];
    let mut dbuf = vec![0u8; 65536];

    shared.net.configure(cfg.addresses.clone(), mtu);
    shared.set_routes(cfg.allowed_ips.clone());
    shared.dns.set_servers(cfg.dns.clone());
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(1024);
    shared.net.set_sink(Some(Arc::new(Sink(tx))));
    let _clear = ClearSink(shared.clone());

    if let TunnResult::WriteToNetwork(p) = tunn.format_handshake_initiation(&mut out, false) {
        let _ = sock.send(p).await;
    }
    let started = Instant::now();
    let mut connected = false;
    let mut last_rx = Instant::now();
    let mut last_data_tx: Option<Instant> = None;
    let mut probes = 0u32;
    let mut last_probe = Instant::now();
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let tunnel_ip = cfg.addresses.first().map(|a| a.addr.to_string());
    let mut recv_errors = 0u32;

    loop {
        tokio::select! {
            _ = crate::service::stopped(stop) => return SessionEnd::Stopped,
            r = sock.recv(&mut rbuf) => {
                let n = match r {
                    Ok(n) => {
                        recv_errors = 0;
                        n
                    }
                    Err(_) => {
                        // E.g. ICMP port unreachable (ConnectionReset on Windows) while the
                        // server is down: wait a little, longer each time, instead of spinning.
                        recv_errors = recv_errors.saturating_add(1);
                        if recv_errors >= RECV_ERRORS_GIVE_UP {
                            return SessionEnd::Retry("The server is unreachable".into());
                        }
                        tokio::select! {
                            _ = crate::service::stopped(stop) => return SessionEnd::Stopped,
                            _ = tokio::time::sleep(recv_backoff(recv_errors)) => {}
                        }
                        continue;
                    }
                };
                let mut res = tunn.decapsulate(Some(endpoint.ip()), &rbuf[..n], &mut dbuf);
                let mut valid = false;
                loop {
                    match res {
                        TunnResult::WriteToNetwork(p) => {
                            valid = true;
                            let _ = sock.send(p).await;
                            res = tunn.decapsulate(None, &[], &mut dbuf);
                            continue;
                        }
                        TunnResult::WriteToTunnelV4(p, _) | TunnResult::WriteToTunnelV6(p, _) => {
                            valid = true;
                            shared.net.inject(p);
                        }
                        TunnResult::Done => valid = true,
                        TunnResult::Err(_) => {}
                    }
                    break;
                }
                if valid {
                    last_rx = Instant::now();
                    probes = 0;
                }
                if !connected && tunn.time_since_last_handshake().is_some() {
                    connected = true;
                    shared.connected(tunnel_ip.clone());
                }
            }
            Some(p) = rx.recv() => {
                if let TunnResult::WriteToNetwork(d) = tunn.encapsulate(&p, &mut out) {
                    let _ = sock.send(d).await;
                }
                if connected && last_data_tx.is_none_or(|t| t < last_rx) {
                    last_data_tx = Some(Instant::now());
                }
            }
            _ = tick.tick() => {
                match tunn.update_timers(&mut out) {
                    TunnResult::WriteToNetwork(d) => { let _ = sock.send(d).await; }
                    TunnResult::Err(boringtun::noise::errors::WireGuardError::ConnectionExpired) => {
                        return SessionEnd::Retry("The connection expired".into());
                    }
                    _ => {}
                }
                if !connected {
                    if started.elapsed() > HANDSHAKE_GIVE_UP {
                        return SessionEnd::Retry("No answer from the server (check the keys and the endpoint)".into());
                    }
                    continue;
                }
                let silent = last_rx.elapsed();
                let sent_since_rx = last_data_tx.is_some_and(|t| t > last_rx);
                if probes == 0 {
                    let due = silent >= PROBE_IDLE
                        || (sent_since_rx && last_data_tx.is_some_and(|t| t.elapsed() >= PROBE_AFTER_SEND));
                    if due {
                        probes = 1;
                        last_probe = Instant::now();
                        if let TunnResult::WriteToNetwork(d) = tunn.format_handshake_initiation(&mut out, true) {
                            let _ = sock.send(d).await;
                        }
                    }
                } else if last_probe.elapsed() >= PROBE_TIMEOUT {
                    if probes >= 2 {
                        return SessionEnd::Retry("The server stopped answering".into());
                    }
                    probes += 1;
                    last_probe = Instant::now();
                    if let TunnResult::WriteToNetwork(d) = tunn.format_handshake_initiation(&mut out, true) {
                        let _ = sock.send(d).await;
                    }
                }
            }
        }
    }
}

/// Removes the WireGuard sink from the stack when the session ends, however it ends.
struct ClearSink(Arc<Shared>);

impl Drop for ClearSink {
    fn drop(&mut self) {
        self.0.net.set_sink(None);
    }
}

/// The endpoint's address through the system resolver (the server must be reachable outside
/// the tunnel). IPv4 first.
async fn resolve(endpoint: &str) -> Result<SocketAddr, String> {
    let Some((host, port)) = parse_endpoint(endpoint) else { return Err("The server address is invalid".into()) };
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    let lookup = tokio::time::timeout(Duration::from_secs(10), tokio::net::lookup_host((host.as_str(), port))).await;
    let addrs: Vec<SocketAddr> = match lookup {
        Ok(Ok(a)) => a.collect(),
        _ => Vec::new(),
    };
    addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or(addrs.first())
        .copied()
        .ok_or_else(|| "The server name could not be resolved".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIV: &str = "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=";
    const PUB: &str = "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=";

    fn good() -> String {
        format!(
            "# Name = Office\n[Interface]\nPrivateKey = {PRIV}\nAddress = 10.66.0.2/32, fd00::2/128\nDNS = 10.77.0.53, corp.example\n\
             MTU = 1380\nPostUp = iptables -A FORWARD -j ACCEPT\n\n[Peer]\nPublicKey = {PUB}\n\
             PresharedKey = {PRIV}\nAllowedIPs = 10.77.0.0/24,\n  AllowedIPs = 0.0.0.0/0\nEndpoint = vpn.example.com:51820\n\
             PersistentKeepalive = 25 # comment\n"
        )
    }

    #[test]
    fn parses_good() {
        let p = parse(&good()).unwrap();
        let c = &p.config;
        assert_eq!(c.endpoint(), "vpn.example.com:51820");
        assert_eq!(c.addresses().len(), 2);
        assert_eq!(c.addresses()[0].to_string(), "10.66.0.2/32");
        assert_eq!(c.dns(), &["10.77.0.53".parse::<IpAddr>().unwrap()]);
        assert_eq!(c.mtu(), 1380);
        assert_eq!(c.allowed_ips().len(), 2);
        assert_eq!(c.persistent_keepalive(), Some(25));
        assert_eq!(c.peer_public_key(), PUB);
        assert!(c.preshared.is_some());
        assert_eq!(p.name.as_deref(), Some("Office"));
        assert!(p.warnings.iter().any(|w| w.contains("PostUp")), "{:?}", p.warnings);
        assert!(p.warnings.iter().any(|w| w.contains("search domains")), "{:?}", p.warnings);
        let dbg = format!("{c:?}");
        assert!(!dbg.contains(PRIV), "{dbg}");
    }

    #[test]
    fn minimal_and_v6_endpoint() {
        let t = format!(
            "[interface]\nprivatekey={PRIV}\naddress=10.0.0.2\n[peer]\npublickey={PUB}\nallowedips=::/0\nendpoint=[2001:db8::1]:51820\n"
        );
        let p = parse(&t).unwrap();
        assert_eq!(p.config.endpoint(), "[2001:db8::1]:51820");
        assert_eq!(p.config.mtu(), DEFAULT_MTU);
        assert_eq!(p.config.persistent_keepalive(), None);
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    }

    #[test]
    fn rejects_bad() {
        let base = good();
        let cases: Vec<(String, &str)> = vec![
            (base.replace(PRIV, "short"), "PrivateKey"),
            (base.replace("Endpoint = vpn.example.com:51820\n", ""), "Endpoint"),
            (base.replace("Endpoint = vpn.example.com:51820", "Endpoint = vpn.example.com"), "Endpoint"),
            (base.replace("Endpoint = vpn.example.com:51820", "Endpoint = host:0"), "Endpoint"),
            (base.replace("Endpoint = vpn.example.com:51820", "Endpoint = a b:1"), "Endpoint"),
            (format!("{base}\n[Peer]\nPublicKey = {PUB}\n"), "one [Peer]"),
            (format!("PrivateKey = {PRIV}\n{base}"), "outside"),
            (base.replace("MTU = 1380", "MTU = 100"), "MTU"),
            (base.replace("Address = 10.66.0.2/32, fd00::2/128", "Address = 10.66.0.2/40"), "Address"),
            (base.replace("[Peer]", "[Peer"), "section"),
            (base.replace("MTU = 1380", "MTU = 1380\nMTU = 1400"), "twice"),
            (base.replace("MTU = 1380", "Jc = 4"), "AmneziaWG"),
            (base.replace("PublicKey", "Garbage"), "PublicKey"),
            ("[Interface]\n".to_owned(), "[Peer]"),
            (base.replace("MTU = 1380", "just words"), "Key = Value"),
        ];
        for (text, needle) in cases {
            let e = parse(&text).unwrap_err().to_string();
            assert!(e.contains(needle), "{needle}: {e}");
            assert!(!e.contains(PRIV));
        }
    }

    #[test]
    fn hostile() {
        // Very many entries, odd bytes, long lines: errors, no panics.
        let many = format!("{}{}", good(), "AllowedIPs = 10.0.0.0/8\n".repeat(2000));
        assert!(parse(&many).is_err());
        let long = format!("[Interface]\nPrivateKey = {}\n", "A".repeat(60000));
        assert!(parse(&long).is_err());
        let odd = "[Interface]\nPrivateKey = \u{202e}\u{0}\n[Peer]\n";
        assert!(parse(odd).is_err());
        assert!(parse("[]\n").is_err());
        assert!(parse("[Interface]\n=\n").is_err());
        let unknown = good().replace("MTU = 1380", "Frobnicate = 1");
        assert!(parse(&unknown).unwrap().warnings.iter().any(|w| w.contains("frobnicate")));
    }

    #[test]
    fn recv_errors_back_off() {
        let v: Vec<u64> = (1..=10).map(|n| recv_backoff(n).as_millis() as u64).collect();
        assert_eq!(v, vec![10, 20, 40, 80, 160, 320, 640, 1000, 1000, 1000]);
    }

    #[test]
    fn endpoints() {
        assert_eq!(parse_endpoint("1.2.3.4:51820"), Some(("1.2.3.4".into(), 51820)));
        assert_eq!(parse_endpoint("[::1]:1"), Some(("::1".into(), 1)));
        assert_eq!(parse_endpoint("::1:1"), None);
        assert_eq!(parse_endpoint("[::1]"), None);
        assert_eq!(parse_endpoint("h:65536"), None);
        assert_eq!(parse_endpoint(":1"), None);
        assert_eq!(parse_endpoint("a/b:1"), None);
    }
}
