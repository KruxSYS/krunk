//! Custom DNS (Krunk 0.6.0, Settings → Privacy → DNS): the resolver model, the text and
//! `sdns://` stamp forms a resolver can be entered as, the presets, and validation. The
//! resolving itself (a loopback DNS-over-HTTPS stub the engine is pointed at, with the
//! user's ordered list behind it) is in the app. Everything here is pure.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

pub const MAX_RESOLVERS: usize = 8;
/// How long one resolver gets before the next one is tried.
pub const RESOLVER_TIMEOUT_MS: u64 = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Windows' resolver (the engine's default: DoH upgrade when the system resolver has it).
    #[default]
    System,
    /// Krunk's resolver list, in order, with fallback; nothing else is ever asked.
    Custom,
}

/// The transport to a resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Udp,
    Tcp,
    /// DNS over TLS (RFC 7858), port 853.
    Tls,
    /// DNS over HTTPS (RFC 8484), HTTP/2.
    Https,
    /// DNS over HTTPS over HTTP/3.
    H3,
    /// DNS over QUIC (RFC 9250), port 853.
    Quic,
    /// Parsed from a stamp but not supported: the entry is kept, shown greyed, and skipped.
    DnsCrypt,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Udp => "UDP",
            Kind::Tcp => "TCP",
            Kind::Tls => "DoT",
            Kind::Https => "DoH",
            Kind::H3 => "DoH3",
            Kind::Quic => "DoQ",
            Kind::DnsCrypt => "DNSCrypt",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Kind::Udp | Kind::Tcp => 53,
            Kind::Tls | Kind::Quic => 853,
            Kind::Https | Kind::H3 => 443,
            Kind::DnsCrypt => 443,
        }
    }

    pub fn supported(self) -> bool {
        self != Kind::DnsCrypt
    }

    fn scheme(self) -> &'static str {
        match self {
            Kind::Udp => "udp",
            Kind::Tcp => "tcp",
            Kind::Tls => "tls",
            Kind::Https => "https",
            Kind::H3 => "h3",
            Kind::Quic => "quic",
            Kind::DnsCrypt => "dnscrypt",
        }
    }
}

/// One resolver of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Resolver {
    pub kind: Kind,
    /// Shown in the list (a preset's name, or the address).
    pub name: String,
    /// The host of the resolver: an IP address, or the name its certificate carries (DoT,
    /// DoH, DoQ).
    pub host: String,
    /// The address to connect to when `host` is a name (a stamp's or preset's bootstrap
    /// address); empty: the name is looked up once through Windows' resolver.
    pub ip: String,
    pub port: u16,
    /// DoH only: the query path.
    pub path: String,
    /// The `sdns://` stamp it came from, if any (kept for the list).
    pub stamp: String,
}

impl Default for Resolver {
    fn default() -> Resolver {
        Resolver {
            kind: Kind::Udp,
            name: String::new(),
            host: String::new(),
            ip: String::new(),
            port: 53,
            path: String::new(),
            stamp: String::new(),
        }
    }
}

impl Resolver {
    /// Parses what the user typed: `1.1.1.1`, `udp://`, `tcp://`, `tls://dns.quad9.net`,
    /// `quic://host`, `h3://host/path`, `https://host/dns-query` or an `sdns://` stamp.
    pub fn parse(text: &str) -> Result<Resolver, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Enter a resolver".into());
        }
        if let Some(stamp) = text.strip_prefix("sdns://") {
            return parse_stamp(stamp);
        }
        let (kind, rest) = match text.split_once("://") {
            Some(("udp", r)) => (Kind::Udp, r),
            Some(("tcp", r)) => (Kind::Tcp, r),
            Some(("tls", r)) | Some(("dot", r)) => (Kind::Tls, r),
            Some(("quic", r)) | Some(("doq", r)) => (Kind::Quic, r),
            Some(("h3", r)) => (Kind::H3, r),
            Some(("https", r)) => (Kind::Https, r),
            Some((s, _)) => return Err(format!("Unknown scheme {s}:// (udp, tcp, tls, quic, h3, https or sdns)")),
            None => (Kind::Udp, text),
        };
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        let (host, port) = split_host_port(authority)?;
        let port = port.unwrap_or(kind.default_port());
        valid_host(&host)?;
        if matches!(kind, Kind::Udp | Kind::Tcp) && host.parse::<IpAddr>().is_err() {
            return Err("Plain DNS needs an IP address".into());
        }
        let path = if matches!(kind, Kind::Https | Kind::H3) {
            if path.is_empty() || path == "/" {
                "/dns-query".to_string()
            } else {
                path.to_string()
            }
        } else if !path.is_empty() && path != "/" {
            return Err("Only DoH resolvers have a path".into());
        } else {
            String::new()
        };
        let mut r = Resolver { kind, host, port, path, ..Resolver::default() };
        r.name = r.address();
        Ok(r)
    }

    /// The address as text (`tls://dns.quad9.net`, `https://host/dns-query`, `1.1.1.1`).
    pub fn address(&self) -> String {
        let port = if self.port == self.kind.default_port() { String::new() } else { format!(":{}", self.port) };
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        match self.kind {
            Kind::Udp => format!("{host}{port}"),
            Kind::Https | Kind::H3 => format!("{}://{host}{port}{}", self.kind.scheme(), self.path),
            k => format!("{}://{host}{port}", k.scheme()),
        }
    }

    /// The socket address to connect to, when known without a lookup.
    pub fn socket_addr(&self) -> Option<SocketAddr> {
        let ip = if !self.ip.is_empty() { &self.ip } else { &self.host };
        if let Ok(a) = ip.parse::<SocketAddr>() {
            return Some(a);
        }
        ip.trim_matches(['[', ']']).parse::<IpAddr>().ok().map(|ip| SocketAddr::new(ip, self.port))
    }

    /// The name the certificate must carry (DoT, DoH, DoQ): the host when it is a name.
    pub fn server_name(&self) -> Option<String> {
        (self.host.parse::<IpAddr>().is_err()).then(|| self.host.clone())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.len() > 64 || self.name.chars().any(char::is_control) {
            return Err("The name is at most 64 characters".into());
        }
        valid_host(&self.host)?;
        if !self.ip.is_empty() && self.socket_addr().is_none() {
            return Err(format!("Not an IP address: {}", self.ip));
        }
        if self.port == 0 {
            return Err("The port is 1 to 65535".into());
        }
        if matches!(self.kind, Kind::Https | Kind::H3) && (!self.path.starts_with('/') || self.path.len() > 256) {
            return Err("A DoH path starts with /".into());
        }
        if self.stamp.len() > 1024 {
            return Err("The stamp is too long".into());
        }
        Ok(())
    }
}

fn split_host_port(authority: &str) -> Result<(String, Option<u16>), String> {
    if authority.is_empty() {
        return Err("Enter a host".into());
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, after)) = rest.split_once(']') else { return Err("Unbalanced [ in the address".into()) };
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p.parse::<u16>().map_err(|_| format!("Not a port: {p}"))?),
            None if after.is_empty() => None,
            None => return Err("Unexpected text after the address".into()),
        };
        return Ok((host.to_string(), port));
    }
    if authority.matches(':').count() > 1 {
        // A bare IPv6 address.
        return Ok((authority.to_string(), None));
    }
    match authority.split_once(':') {
        Some((h, p)) => Ok((h.to_string(), Some(p.parse::<u16>().map_err(|_| format!("Not a port: {p}"))?))),
        None => Ok((authority.to_string(), None)),
    }
}

fn valid_host(host: &str) -> Result<(), String> {
    if host.is_empty() || host.len() > 253 {
        return Err("Enter a host".into());
    }
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    let ok = host.split('.').all(|l| {
        !l.is_empty()
            && l.len() <= 63
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !l.starts_with('-')
    });
    if ok {
        Ok(())
    } else {
        Err(format!("Not a host name: {host}"))
    }
}

/// Decodes a DNS stamp (https://dnscrypt.info/stamps-specifications/): plain DNS, DNSCrypt
/// (parsed, not supported), DoH, DoT and DoQ. Relays and oblivious DoH are refused.
pub fn parse_stamp(encoded: &str) -> Result<Resolver, String> {
    use base64ct::{Base64UrlUnpadded, Encoding};
    let bytes = Base64UrlUnpadded::decode_vec(encoded.trim_end_matches('='))
        .map_err(|_| "Not a stamp (sdns://… is base64url)".to_string())?;
    let mut cur = Cursor { bytes: &bytes, pos: 0 };
    let proto = cur.byte()?;
    let _props = cur.take(8)?;
    let kind = match proto {
        0x00 => Kind::Udp,
        0x01 => Kind::DnsCrypt,
        0x02 => Kind::Https,
        0x03 => Kind::Tls,
        0x04 => Kind::Quic,
        0x05 => return Err("Oblivious DoH stamps are not supported".into()),
        0x81 | 0x85 => return Err("Relay stamps are not supported".into()),
        p => return Err(format!("Unknown stamp protocol 0x{p:02x}")),
    };
    let addr = cur.lp_str()?;
    let mut r = Resolver { kind, stamp: format!("sdns://{encoded}"), ..Resolver::default() };
    let (addr_host, addr_port) = if addr.is_empty() { (String::new(), None) } else { split_host_port(&addr)? };
    match kind {
        Kind::Udp => {
            if addr_host.is_empty() {
                return Err("The stamp has no address".into());
            }
            r.host = addr_host;
            r.port = addr_port.unwrap_or(53);
        }
        Kind::DnsCrypt => {
            let _pk = cur.lp()?;
            let provider = cur.lp_str()?;
            r.host = if addr_host.is_empty() { provider.clone() } else { addr_host };
            r.port = addr_port.unwrap_or(443);
            r.name = format!("{provider} (DNSCrypt, not supported)");
            return Ok(r);
        }
        Kind::Https | Kind::Tls | Kind::Quic => {
            let _hashes = cur.vlp()?;
            let hostname = cur.lp_str()?;
            let (name_host, name_port) = split_host_port(&hostname)?;
            if kind == Kind::Https {
                r.path = cur.lp_str()?;
                if r.path.is_empty() {
                    r.path = "/dns-query".into();
                }
            }
            r.host = if name_host.is_empty() { addr_host.clone() } else { name_host };
            r.port = name_port.or(addr_port).unwrap_or(kind.default_port());
            if !addr_host.is_empty() && addr_host != r.host {
                r.ip = match addr_port {
                    Some(p) if p != r.port => format!("{addr_host}:{p}"),
                    _ => addr_host,
                };
            }
        }
        _ => unreachable!(),
    }
    valid_host(&r.host)?;
    r.name = r.address();
    Ok(r)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.bytes.get(self.pos).ok_or("The stamp is truncated")?;
        self.pos += 1;
        Ok(b)
    }
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.pos + n;
        let s = self.bytes.get(self.pos..end).ok_or("The stamp is truncated")?;
        self.pos = end;
        Ok(s)
    }
    fn lp(&mut self) -> Result<Vec<u8>, String> {
        let n = self.byte()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn lp_str(&mut self) -> Result<String, String> {
        String::from_utf8(self.lp()?).map_err(|_| "The stamp has invalid text".to_string())
    }
    /// A sequence of length-prefixed items; the high bit of a length says more follow.
    fn vlp(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let mut items = Vec::new();
        loop {
            let n = self.byte()?;
            let more = n & 0x80 != 0;
            let len = (n & 0x7f) as usize;
            items.push(self.take(len)?.to_vec());
            if !more {
                return Ok(items);
            }
        }
    }
}

/// Standard base64 (the engine's SPKI switch wants it).
pub fn base64(bytes: &[u8]) -> String {
    use base64ct::{Base64, Encoding};
    Base64::encode_string(bytes)
}

/// Decodes the `dns=` parameter of a DoH GET (base64url, no padding).
pub fn base64url_decode(text: &str) -> Result<Vec<u8>, String> {
    use base64ct::{Base64UrlUnpadded, Encoding};
    Base64UrlUnpadded::decode_vec(text.trim_end_matches('=')).map_err(|e| e.to_string())
}

/// The REFUSED answer to a DNS query (0.14.0: the loopback stub while the VPN runs): the
/// query's id, opcode, RD bit and question, rcode 5, no records. `None` for a packet that is
/// not a query.
pub fn refused_answer(query: &[u8]) -> Option<Vec<u8>> {
    if query.len() < 12 || query[2] & 0x80 != 0 {
        return None;
    }
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    // The question (plain labels; a query has no compression), when there is exactly one.
    let question = (qdcount == 1)
        .then(|| {
            let mut i = 12;
            loop {
                let len = usize::from(*query.get(i)?);
                if len == 0 {
                    break;
                }
                if len & 0xc0 != 0 {
                    return None;
                }
                i += 1 + len;
            }
            query.get(12..i + 5)
        })
        .flatten();
    let mut out = Vec::with_capacity(12 + question.map_or(0, <[u8]>::len));
    out.extend_from_slice(&query[0..2]);
    // QR, the query's opcode and RD; RA 0; rcode 5 (REFUSED).
    out.push(0x80 | (query[2] & 0x79));
    out.push(5);
    out.extend_from_slice(&[0, u8::from(question.is_some()), 0, 0, 0, 0, 0, 0]);
    if let Some(q) = question {
        out.extend_from_slice(q);
    }
    Some(out)
}

/// A preset of the picker.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub address: &'static str,
    /// The bootstrap address of a named resolver.
    pub ip: &'static str,
}

/// Well-known public resolvers; the IPs let them be reached without any other lookup.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "cloudflare-doh",
        name: "Cloudflare (DoH)",
        address: "https://cloudflare-dns.com/dns-query",
        ip: "1.1.1.1",
    },
    Preset { id: "cloudflare-dot", name: "Cloudflare (DoT)", address: "tls://one.one.one.one", ip: "1.1.1.1" },
    Preset { id: "cloudflare-udp", name: "Cloudflare (plain UDP)", address: "1.1.1.1", ip: "" },
    Preset { id: "quad9-doh", name: "Quad9 (DoH)", address: "https://dns.quad9.net/dns-query", ip: "9.9.9.9" },
    Preset { id: "quad9-dot", name: "Quad9 (DoT)", address: "tls://dns.quad9.net", ip: "9.9.9.9" },
    Preset { id: "google-doh", name: "Google (DoH)", address: "https://dns.google/dns-query", ip: "8.8.8.8" },
    Preset { id: "google-dot", name: "Google (DoT)", address: "tls://dns.google", ip: "8.8.8.8" },
    Preset {
        id: "adguard-doh",
        name: "AdGuard (DoH)",
        address: "https://dns.adguard-dns.com/dns-query",
        ip: "94.140.14.14",
    },
    Preset { id: "adguard-doq", name: "AdGuard (DoQ)", address: "quic://dns.adguard-dns.com", ip: "94.140.14.14" },
    Preset {
        id: "mullvad-doh",
        name: "Mullvad (DoH)",
        address: "https://dns.mullvad.net/dns-query",
        ip: "194.242.2.2",
    },
];

impl Preset {
    pub fn resolver(&self) -> Resolver {
        let mut r = Resolver::parse(self.address).expect("preset addresses parse");
        r.name = self.name.to_string();
        r.ip = self.ip.to_string();
        r
    }
}

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// The DNS settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Dns {
    pub mode: Mode,
    pub resolvers: Vec<Resolver>,
}

impl Dns {
    /// Custom mode with at least one usable resolver.
    pub fn active(&self) -> bool {
        self.mode == Mode::Custom && self.resolvers.iter().any(|r| r.kind.supported())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.resolvers.len() > MAX_RESOLVERS {
            return Err(format!("At most {MAX_RESOLVERS} resolvers"));
        }
        for r in &self.resolvers {
            r.validate().map_err(|e| format!("{}: {e}", r.address()))?;
        }
        if self.mode == Mode::Custom && !self.resolvers.iter().any(|r| r.kind.supported()) {
            return Err("Custom DNS needs at least one supported resolver".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_answers_echo_the_question() {
        // id 0x1234, RD, one question: example.com A IN, plus an OPT record.
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 1];
        q.extend_from_slice(b"\x07example\x03com\x00\x00\x01\x00\x01");
        q.extend_from_slice(&[0, 0, 41, 4, 0xd0, 0, 0, 0, 0, 0, 0]);
        let a = refused_answer(&q).unwrap();
        assert_eq!(&a[..12], &[0x12, 0x34, 0x81, 0x05, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&a[12..], b"\x07example\x03com\x00\x00\x01\x00\x01");
        // An answer is not a query; short and odd packets get no question back.
        let mut answer = q.clone();
        answer[2] |= 0x80;
        assert_eq!(refused_answer(&answer), None);
        assert_eq!(refused_answer(&q[..11]), None);
        let truncated = &q[..16];
        assert_eq!(refused_answer(truncated).unwrap(), vec![0x12, 0x34, 0x81, 0x05, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mut two = q.clone();
        two[5] = 2;
        assert_eq!(refused_answer(&two).unwrap()[5], 0);
        let mut pointer = q[..12].to_vec();
        pointer.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1]);
        assert_eq!(refused_answer(&pointer).unwrap().len(), 12);
    }

    #[test]
    fn text_forms() {
        let r = Resolver::parse("1.1.1.1").unwrap();
        assert_eq!((r.kind, r.host.as_str(), r.port), (Kind::Udp, "1.1.1.1", 53));
        assert_eq!(r.socket_addr().unwrap().to_string(), "1.1.1.1:53");
        let r = Resolver::parse("tcp://[2606:4700:4700::1111]:5353").unwrap();
        assert_eq!((r.kind, r.port), (Kind::Tcp, 5353));
        assert_eq!(r.address(), "tcp://[2606:4700:4700::1111]:5353");
        let r = Resolver::parse("tls://dns.quad9.net").unwrap();
        assert_eq!((r.kind, r.port, r.server_name().as_deref()), (Kind::Tls, 853, Some("dns.quad9.net")));
        assert!(r.socket_addr().is_none(), "a name needs a lookup or a bootstrap ip");
        let r = Resolver::parse("https://dns.google").unwrap();
        assert_eq!((r.kind, r.path.as_str(), r.port), (Kind::Https, "/dns-query", 443));
        assert_eq!(r.address(), "https://dns.google/dns-query");
        let r = Resolver::parse("h3://dns.adguard-dns.com/dns-query").unwrap();
        assert_eq!(r.kind, Kind::H3);
        let r = Resolver::parse("quic://94.140.14.14").unwrap();
        assert_eq!((r.kind, r.port), (Kind::Quic, 853));
        assert!(r.server_name().is_none());
        for bad in [
            "",
            "ftp://x",
            "udp://dns.google",
            "tls://dns.quad9.net/path",
            "https://",
            "tls://bad host",
            "1.1.1.1:99999",
        ] {
            assert!(Resolver::parse(bad).is_err(), "{bad}");
        }
    }

    /// Builds a stamp from its parts (the spec's layout) for the tests.
    fn stamp(proto: u8, parts: &[&[u8]], vlp_at: Option<usize>) -> String {
        use base64ct::{Base64UrlUnpadded, Encoding};
        let mut b = vec![proto];
        b.extend_from_slice(&7u64.to_le_bytes());
        for (i, p) in parts.iter().enumerate() {
            if Some(i) == vlp_at {
                // One hash entry (or none when empty).
                b.push(p.len() as u8);
            } else {
                b.push(p.len() as u8);
            }
            b.extend_from_slice(p);
        }
        Base64UrlUnpadded::encode_string(&b)
    }

    #[test]
    fn stamps() {
        // Cloudflare's published DoH stamp.
        let r = parse_stamp("AgcAAAAAAAAABzEuMS4xLjEAEmRucy5jbG91ZGZsYXJlLmNvbQovZG5zLXF1ZXJ5").unwrap();
        assert_eq!(
            (r.kind, r.host.as_str(), r.ip.as_str(), r.path.as_str(), r.port),
            (Kind::Https, "dns.cloudflare.com", "1.1.1.1", "/dns-query", 443)
        );
        assert_eq!(r.socket_addr().unwrap().to_string(), "1.1.1.1:443");
        assert!(r.stamp.starts_with("sdns://Agc"));
        let same = Resolver::parse("sdns://AgcAAAAAAAAABzEuMS4xLjEAEmRucy5jbG91ZGZsYXJlLmNvbQovZG5zLXF1ZXJ5").unwrap();
        assert_eq!(same.host, r.host);
        // DoT with a port in the address and a hash.
        let s = stamp(0x03, &[b"9.9.9.9:853", &[0xab; 32], b"dns.quad9.net"], Some(1));
        let r = parse_stamp(&s).unwrap();
        assert_eq!((r.kind, r.host.as_str(), r.ip.as_str(), r.port), (Kind::Tls, "dns.quad9.net", "9.9.9.9", 853));
        // DoQ without an address: the name is looked up.
        let s = stamp(0x04, &[b"", b"", b"dns.adguard-dns.com"], Some(1));
        let r = parse_stamp(&s).unwrap();
        assert_eq!((r.kind, r.ip.as_str(), r.port), (Kind::Quic, "", 853));
        // Plain DNS.
        let s = stamp(0x00, &[b"8.8.8.8"], None);
        let r = parse_stamp(&s).unwrap();
        assert_eq!((r.kind, r.port, r.host.as_str()), (Kind::Udp, 53, "8.8.8.8"));
        // DNSCrypt is parsed but marked unsupported.
        let s = stamp(0x01, &[b"176.103.130.130:5443", &[1u8; 32], b"2.dnscrypt.default.ns1.adguard.com"], None);
        let r = parse_stamp(&s).unwrap();
        assert_eq!(r.kind, Kind::DnsCrypt);
        assert!(!r.kind.supported() && r.name.contains("not supported"));
        // Refused forms.
        assert!(parse_stamp("not base64!").is_err());
        assert!(parse_stamp(&stamp(0x81, &[b"1.2.3.4"], None)).unwrap_err().contains("Relay"));
        assert!(parse_stamp(&stamp(0x05, &[b"", b"", b"x", b"/"], Some(1))).unwrap_err().contains("Oblivious"));
        assert!(parse_stamp("Ag").is_err(), "truncated");
    }

    #[test]
    fn presets_and_settings() {
        for p in PRESETS {
            let r = p.resolver();
            assert!(r.validate().is_ok(), "{}", p.id);
            if !p.ip.is_empty() {
                assert!(r.socket_addr().is_some(), "{} reaches its address without a lookup", p.id);
            }
        }
        assert_eq!(preset("quad9-dot").unwrap().resolver().kind, Kind::Tls);
        let mut d = Dns::default();
        assert!(!d.active() && d.validate().is_ok());
        d.mode = Mode::Custom;
        assert!(d.validate().is_err(), "custom needs a resolver");
        d.resolvers.push(preset("cloudflare-doh").unwrap().resolver());
        assert!(d.active() && d.validate().is_ok());
        d.resolvers[0].port = 0;
        assert!(d.validate().unwrap_err().contains("port"));
        let json = serde_json::to_string(&Dns::default()).unwrap();
        assert!(json.contains("\"mode\":\"system\""));
        let back: Dns = serde_json::from_str(
            r#"{"mode":"custom","resolvers":[{"kind":"tls","host":"dns.quad9.net","ip":"9.9.9.9","port":853}]}"#,
        )
        .unwrap();
        assert_eq!(back.resolvers[0].kind, Kind::Tls);
        assert!(back.active());
    }
}
