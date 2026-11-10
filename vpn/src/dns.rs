//! Name resolution through the tunnel: A/AAAA queries to the config's or the pushed DNS
//! servers over UDP inside the tunnel (smoltcp), TCP when the answer is truncated or UDP gets
//! no answer, and a small TTL cache. With no DNS server configured it asks 1.1.1.1, still
//! through the tunnel. The system resolver is never used for pages.

use crate::net::Netstack;
use std::collections::HashMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const TYPE_A: u16 = 1;
pub const TYPE_AAAA: u16 = 28;
const TYPE_CNAME: u16 = 5;
const TYPE_OPT: u16 = 41;
const CLASS_IN: u16 = 1;
/// The fallback resolver when neither the config nor the server names one.
pub const FALLBACK: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
const CACHE_MAX: usize = 1024;
const MIN_TTL: u32 = 5;
const MAX_TTL: u32 = 600;
const NEGATIVE_TTL: Duration = Duration::from_secs(30);
const UDP_TIMEOUT: Duration = Duration::from_millis(2000);
const TCP_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DnsError {
    /// Not a name we would ask for.
    BadName,
    /// The name does not exist (NXDOMAIN) or has no address.
    NotFound,
    /// The server answered with an error, or nonsense.
    Server(&'static str),
    /// No server answered.
    Timeout,
}

impl fmt::Display for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DnsError::BadName => f.write_str("invalid host name"),
            DnsError::NotFound => f.write_str("the name does not exist"),
            DnsError::Server(m) => write!(f, "DNS error: {m}"),
            DnsError::Timeout => f.write_str("the DNS servers did not answer"),
        }
    }
}

impl std::error::Error for DnsError {}

/// Lowercases and checks a host name (letters, digits, `-`, `_`, labels of 1–63, at most 253).
pub fn normalize(name: &str) -> Option<String> {
    let name = name.strip_suffix('.').unwrap_or(name);
    if name.is_empty() || name.len() > 253 {
        return None;
    }
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        if !label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return None;
        }
    }
    Some(name.to_ascii_lowercase())
}

/// A query for `name` (already normalised) with recursion desired and an EDNS0 OPT record
/// advertising 1232-byte UDP answers.
pub fn build_query(id: u16, name: &str, qtype: u16) -> Result<Vec<u8>, DnsError> {
    let name = normalize(name).ok_or(DnsError::BadName)?;
    let mut q = Vec::with_capacity(name.len() + 30);
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 1]);
    for label in name.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&qtype.to_be_bytes());
    q.extend_from_slice(&CLASS_IN.to_be_bytes());
    // OPT: root name, type 41, class = UDP payload size, TTL 0, no data.
    q.extend_from_slice(&[0]);
    q.extend_from_slice(&TYPE_OPT.to_be_bytes());
    q.extend_from_slice(&1232u16.to_be_bytes());
    q.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    Ok(q)
}

/// A parsed answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub addrs: Vec<IpAddr>,
    /// The smallest TTL of the records used.
    pub ttl: u32,
    /// The TC bit: retry over TCP.
    pub truncated: bool,
}

fn u16_at(p: &[u8], i: usize) -> Result<u16, DnsError> {
    p.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]])).ok_or(DnsError::Server("short packet"))
}

/// Reads a (possibly compressed) name at `pos`; returns it lowercased and the position after it.
fn read_name(p: &[u8], mut pos: usize) -> Result<(String, usize), DnsError> {
    let bad = DnsError::Server("bad name");
    let mut out = String::new();
    let mut end = None;
    let mut jumps = 0;
    loop {
        let len = *p.get(pos).ok_or(bad.clone())? as usize;
        if len & 0xc0 == 0xc0 {
            let lo = *p.get(pos + 1).ok_or(bad.clone())? as usize;
            if end.is_none() {
                end = Some(pos + 2);
            }
            jumps += 1;
            if jumps > 16 {
                return Err(bad);
            }
            pos = ((len & 0x3f) << 8) | lo;
            continue;
        }
        if len & 0xc0 != 0 {
            return Err(bad);
        }
        if len == 0 {
            return Ok((out, end.unwrap_or(pos + 1)));
        }
        let label = p.get(pos + 1..pos + 1 + len).ok_or(bad.clone())?;
        if !out.is_empty() {
            out.push('.');
        }
        for &b in label {
            out.push(char::from(b.to_ascii_lowercase()));
        }
        if out.len() > 255 {
            return Err(bad);
        }
        pos += 1 + len;
    }
}

/// Parses an answer to the query `(id, name, qtype)`; follows CNAMEs within the answer.
pub fn parse_response(id: u16, name: &str, qtype: u16, p: &[u8]) -> Result<Answer, DnsError> {
    let name = normalize(name).ok_or(DnsError::BadName)?;
    if p.len() < 12 {
        return Err(DnsError::Server("short packet"));
    }
    if u16_at(p, 0)? != id {
        return Err(DnsError::Server("wrong id"));
    }
    let flags = u16_at(p, 2)?;
    if flags & 0x8000 == 0 || (flags >> 11) & 0xf != 0 {
        return Err(DnsError::Server("not an answer"));
    }
    let truncated = flags & 0x0200 != 0;
    match flags & 0xf {
        0 => {}
        3 => return Err(DnsError::NotFound),
        2 => return Err(DnsError::Server("server failure")),
        5 => return Err(DnsError::Server("refused")),
        _ => return Err(DnsError::Server("error")),
    }
    let qd = u16_at(p, 4)?;
    let an = u16_at(p, 6)?;
    if qd != 1 {
        return Err(DnsError::Server("bad question count"));
    }
    let (qname, mut pos) = read_name(p, 12)?;
    if qname != name || u16_at(p, pos)? != qtype || u16_at(p, pos + 2)? != CLASS_IN {
        return Err(DnsError::Server("answer for another question"));
    }
    pos += 4;
    if truncated {
        return Ok(Answer { addrs: Vec::new(), ttl: 0, truncated });
    }
    let mut owners = vec![name];
    let mut addrs = Vec::new();
    let mut ttl = u32::MAX;
    for _ in 0..an.min(64) {
        let (owner, next) = read_name(p, pos)?;
        let rtype = u16_at(p, next)?;
        let class = u16_at(p, next + 2)?;
        let rttl = p
            .get(next + 4..next + 8)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or(DnsError::Server("short packet"))?;
        let rdlen = usize::from(u16_at(p, next + 8)?);
        let rdata_at = next + 10;
        let rdata = p.get(rdata_at..rdata_at + rdlen).ok_or(DnsError::Server("short packet"))?;
        pos = rdata_at + rdlen;
        if class != CLASS_IN || !owners.contains(&owner) {
            continue;
        }
        match rtype {
            TYPE_CNAME => {
                let (target, _) = read_name(p, rdata_at)?;
                if owners.len() < 16 && !owners.contains(&target) {
                    owners.push(target);
                }
                ttl = ttl.min(rttl);
            }
            TYPE_A if qtype == TYPE_A && rdlen == 4 => {
                addrs.push(IpAddr::V4(Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3])));
                ttl = ttl.min(rttl);
            }
            TYPE_AAAA if qtype == TYPE_AAAA && rdlen == 16 => {
                let b: [u8; 16] = rdata.try_into().map_err(|_| DnsError::Server("bad AAAA"))?;
                addrs.push(IpAddr::V6(Ipv6Addr::from(b)));
                ttl = ttl.min(rttl);
            }
            _ => {}
        }
    }
    if addrs.is_empty() {
        return Err(DnsError::NotFound);
    }
    Ok(Answer { addrs, ttl: ttl.clamp(MIN_TTL, MAX_TTL), truncated: false })
}

struct Entry {
    result: Result<Vec<IpAddr>, DnsError>,
    expires: Instant,
}

/// The resolver: servers plus cache.
pub struct Resolver {
    servers: RwLock<Vec<IpAddr>>,
    cache: Mutex<HashMap<String, Entry>>,
}

impl Default for Resolver {
    fn default() -> Self {
        Resolver::new()
    }
}

impl Resolver {
    pub fn new() -> Resolver {
        Resolver { servers: RwLock::new(vec![FALLBACK]), cache: Mutex::new(HashMap::new()) }
    }

    /// Sets the servers (at most four are used); none means [`FALLBACK`]. Clears the cache
    /// when they change.
    pub fn set_servers(&self, mut servers: Vec<IpAddr>) {
        servers.truncate(4);
        if servers.is_empty() {
            servers.push(FALLBACK);
        }
        let mut cur = self.servers.write().unwrap_or_else(PoisonError::into_inner);
        if *cur != servers {
            *cur = servers;
            self.cache.lock().unwrap_or_else(PoisonError::into_inner).clear();
        }
    }

    /// Forgets every cached answer (at the start of each tunnel session).
    pub fn clear_cache(&self) {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    pub fn servers(&self) -> Vec<IpAddr> {
        self.servers.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Resolves `host` through the tunnel: IPv4 addresses, and IPv6 ones when the tunnel has
    /// an IPv6 address and there is no IPv4 one. Literal IPs are returned as they are.
    pub async fn resolve(&self, net: &Netstack, host: &str) -> Result<Vec<IpAddr>, DnsError> {
        let host = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(vec![ip]);
        }
        let name = normalize(host).ok_or(DnsError::BadName)?;
        if let Some(r) = self.cached(&name) {
            return r;
        }
        let mut result = self.query(net, &name, TYPE_A).await;
        if net.has_ipv6() && matches!(result, Err(DnsError::NotFound)) {
            if let Ok(v6) = self.query(net, &name, TYPE_AAAA).await {
                result = Ok(v6);
            }
        }
        let (result, ttl) = match result {
            Ok(a) => (Ok(a.addrs), Duration::from_secs(u64::from(a.ttl))),
            Err(DnsError::NotFound) => (Err(DnsError::NotFound), NEGATIVE_TTL),
            Err(e) => return Err(e),
        };
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= CACHE_MAX {
            let now = Instant::now();
            cache.retain(|_, e| e.expires > now);
            if cache.len() >= CACHE_MAX {
                cache.clear();
            }
        }
        cache.insert(name, Entry { result: result.clone(), expires: Instant::now() + ttl });
        result
    }

    fn cached(&self, name: &str) -> Option<Result<Vec<IpAddr>, DnsError>> {
        let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.get(name).filter(|e| e.expires > Instant::now()).map(|e| e.result.clone())
    }

    async fn query(&self, net: &Netstack, name: &str, qtype: u16) -> Result<Answer, DnsError> {
        let servers = self.servers();
        let mut last = DnsError::Timeout;
        // UDP: two rounds over the servers; a server that answers with an error ends it.
        for _round in 0..2 {
            if last != DnsError::Timeout {
                return Err(last);
            }
            for &server in &servers {
                let id = u16::from_le_bytes(crate::random::<2>());
                let q = build_query(id, name, qtype)?;
                let Ok(reply) = net.udp_exchange(SocketAddr::new(server, 53), &q, UDP_TIMEOUT).await else {
                    continue;
                };
                match parse_response(id, name, qtype, &reply) {
                    Ok(a) if a.truncated => return self.query_tcp(net, server, name, qtype).await,
                    Ok(a) => return Ok(a),
                    Err(DnsError::NotFound) => return Err(DnsError::NotFound),
                    Err(e) => last = e,
                }
            }
        }
        if last != DnsError::Timeout {
            return Err(last);
        }
        // Nobody answered over UDP: TCP to the first server.
        match self.query_tcp(net, servers[0], name, qtype).await {
            Ok(a) => Ok(a),
            Err(DnsError::Timeout) => Err(last),
            Err(e) => Err(e),
        }
    }

    async fn query_tcp(&self, net: &Netstack, server: IpAddr, name: &str, qtype: u16) -> Result<Answer, DnsError> {
        let id = u16::from_le_bytes(crate::random::<2>());
        let q = build_query(id, name, qtype)?;
        let exchange = async {
            let mut s = net.tcp_connect(SocketAddr::new(server, 53), TCP_TIMEOUT).await.ok()?;
            let mut msg = (q.len() as u16).to_be_bytes().to_vec();
            msg.extend_from_slice(&q);
            s.write_all(&msg).await.ok()?;
            let mut len = [0u8; 2];
            s.read_exact(&mut len).await.ok()?;
            let mut reply = vec![0u8; usize::from(u16::from_be_bytes(len))];
            s.read_exact(&mut reply).await.ok()?;
            let _ = s.shutdown().await;
            Some(reply)
        };
        let reply = tokio::time::timeout(TCP_TIMEOUT * 2, exchange).await.ok().flatten().ok_or(DnsError::Timeout)?;
        let a = parse_response(id, name, qtype, &reply)?;
        if a.truncated {
            return Err(DnsError::Server("truncated over TCP"));
        }
        Ok(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A response to `q` with the given answer records (name pointer to the question).
    fn response(q: &[u8], rcode: u8, answers: &[(u16, u32, Vec<u8>)]) -> Vec<u8> {
        let qend = q.len() - 11; // without the OPT record
        let mut r = q[..qend].to_vec();
        r[2] = 0x81;
        r[3] = 0x80 | rcode;
        r[6..8].copy_from_slice(&(answers.len() as u16).to_be_bytes());
        r[10..12].copy_from_slice(&[0, 0]);
        for (t, ttl, data) in answers {
            r.extend_from_slice(&[0xc0, 12]);
            r.extend_from_slice(&t.to_be_bytes());
            r.extend_from_slice(&CLASS_IN.to_be_bytes());
            r.extend_from_slice(&ttl.to_be_bytes());
            r.extend_from_slice(&(data.len() as u16).to_be_bytes());
            r.extend_from_slice(data);
        }
        r
    }

    #[test]
    fn query_bytes() {
        let q = build_query(0x1234, "Inside.Test.", TYPE_A).unwrap();
        assert_eq!(&q[..12], &[0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1]);
        assert_eq!(&q[12..25], b"\x06inside\x04test\x00");
        assert_eq!(&q[25..29], &[0, 1, 0, 1]);
        assert_eq!(&q[29..], &[0, 0, 41, 4, 208, 0, 0, 0, 0, 0, 0]);
        assert_eq!(build_query(1, "a..b", TYPE_A), Err(DnsError::BadName));
        assert_eq!(build_query(1, &"a".repeat(64), TYPE_A), Err(DnsError::BadName));
        assert_eq!(build_query(1, "a b", TYPE_A), Err(DnsError::BadName));
        assert_eq!(build_query(1, "", TYPE_A), Err(DnsError::BadName));
    }

    #[test]
    fn parses_answers() {
        let q = build_query(7, "inside.test", TYPE_A).unwrap();
        let r = response(&q, 0, &[(TYPE_A, 300, vec![10, 77, 0, 10]), (TYPE_A, 60, vec![10, 77, 0, 11])]);
        let a = parse_response(7, "inside.test", TYPE_A, &r).unwrap();
        assert_eq!(a.addrs, vec!["10.77.0.10".parse::<IpAddr>().unwrap(), "10.77.0.11".parse().unwrap()]);
        assert_eq!(a.ttl, 60);
        assert_eq!(parse_response(8, "inside.test", TYPE_A, &r).unwrap_err(), DnsError::Server("wrong id"));
        assert!(parse_response(7, "other.test", TYPE_A, &r).is_err());
        assert_eq!(parse_response(7, "inside.test", TYPE_A, &response(&q, 3, &[])).unwrap_err(), DnsError::NotFound);
        assert_eq!(parse_response(7, "inside.test", TYPE_A, &response(&q, 0, &[])).unwrap_err(), DnsError::NotFound);
        assert!(parse_response(7, "inside.test", TYPE_A, &response(&q, 2, &[])).is_err());
        // Wrong-length A data is skipped; TTL 0 is raised to the minimum.
        let r = response(&q, 0, &[(TYPE_A, 0, vec![1, 2, 3]), (TYPE_A, 0, vec![1, 2, 3, 4])]);
        let a = parse_response(7, "inside.test", TYPE_A, &r).unwrap();
        assert_eq!(a.addrs.len(), 1);
        assert_eq!(a.ttl, MIN_TTL);
        // Truncated.
        let mut t = response(&q, 0, &[]);
        t[2] |= 0x02;
        assert!(parse_response(7, "inside.test", TYPE_A, &t).unwrap().truncated);
        // AAAA.
        let q6 = build_query(9, "inside.test", TYPE_AAAA).unwrap();
        let r6 = response(&q6, 0, &[(TYPE_AAAA, 30, vec![0xfd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])]);
        assert_eq!(
            parse_response(9, "inside.test", TYPE_AAAA, &r6).unwrap().addrs,
            vec!["fd00::1".parse::<IpAddr>().unwrap()]
        );
    }

    #[test]
    fn follows_cname_only_in_chain() {
        let q = build_query(1, "www.test", TYPE_A).unwrap();
        let mut r = response(&q, 0, &[]);
        // www.test CNAME cdn.test; cdn.test A 1.2.3.4; evil.test A 6.6.6.6
        let answers: Vec<Vec<u8>> = vec![
            [&[0xc0, 12][..], &[0, 5, 0, 1, 0, 0, 0, 60, 0, 10], b"\x03cdn\x04test\x00"].concat(),
            [&b"\x03cdn\x04test\x00"[..], &[0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 1, 2, 3, 4]].concat(),
            [&b"\x04evil\x04test\x00"[..], &[0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 6, 6, 6, 6]].concat(),
        ];
        r[6..8].copy_from_slice(&3u16.to_be_bytes());
        for a in answers {
            r.extend_from_slice(&a);
        }
        let a = parse_response(1, "www.test", TYPE_A, &r).unwrap();
        assert_eq!(a.addrs, vec!["1.2.3.4".parse::<IpAddr>().unwrap()]);
    }

    #[test]
    fn hostile_packets() {
        let q = build_query(1, "a.test", TYPE_A).unwrap();
        // A compression loop in the answer.
        let mut r = response(&q, 0, &[]);
        r[6..8].copy_from_slice(&1u16.to_be_bytes());
        let at = r.len();
        r.extend_from_slice(&[0xc0, at as u8, 0, 1, 0, 1, 0, 0, 0, 1, 0, 4, 1, 1, 1, 1]);
        assert!(parse_response(1, "a.test", TYPE_A, &r).is_err());
        // Truncated packets of every length: errors, no panics.
        let good = response(&q, 0, &[(TYPE_A, 1, vec![1, 1, 1, 1])]);
        for n in 0..good.len() {
            assert!(parse_response(1, "a.test", TYPE_A, &good[..n]).is_err());
        }
        // A query echoed back (QR=0) is not an answer.
        assert!(parse_response(1, "a.test", TYPE_A, &q).is_err());
        // An answer with an rdlength past the end.
        let mut bad = good.clone();
        let l = bad.len();
        bad[l - 6..l - 4].copy_from_slice(&[0xff, 0xff]);
        assert!(parse_response(1, "a.test", TYPE_A, &bad).is_err());
    }

    #[test]
    fn names() {
        assert_eq!(normalize("Example.COM.").as_deref(), Some("example.com"));
        assert_eq!(normalize("_srv.x-y.test").as_deref(), Some("_srv.x-y.test"));
        assert!(normalize("ex ample.com").is_none());
        assert!(normalize("é.com").is_none());
        assert!(normalize(&format!("{}.com", "a.".repeat(130))).is_none());
    }

    #[test]
    fn servers_default_and_cache_clear() {
        let r = Resolver::new();
        assert_eq!(r.servers(), vec![FALLBACK]);
        r.set_servers(vec![]);
        assert_eq!(r.servers(), vec![FALLBACK]);
        r.set_servers(vec!["10.77.0.53".parse().unwrap()]);
        assert_eq!(r.servers(), vec!["10.77.0.53".parse::<IpAddr>().unwrap()]);
        let entry = || Entry { result: Err(DnsError::NotFound), expires: Instant::now() + Duration::from_secs(60) };
        r.cache.lock().unwrap().insert("a.test".into(), entry());
        assert_eq!(r.cached("a.test"), Some(Err(DnsError::NotFound)));
        // The same servers again keep the cache; a new session clears it.
        r.set_servers(vec!["10.77.0.53".parse().unwrap()]);
        assert!(r.cached("a.test").is_some());
        r.clear_cache();
        assert_eq!(r.cached("a.test"), None);
    }
}
