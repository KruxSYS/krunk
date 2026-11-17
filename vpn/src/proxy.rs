//! The local HTTP proxy on `127.0.0.1:<random>`.
//!
//! - `CONNECT host:port` opens a TCP connection through the tunnel and pipes both ways.
//! - Absolute-form requests (`GET http://host/path`, any method) are rewritten to
//!   origin-form, lose their hop-by-hop headers and go over a tunnel TCP connection, one
//!   request per connection (`Connection: close` both ways).
//! - Names resolve inside the tunnel ([`crate::dns`]).
//! - Fail-closed: while the tunnel is down every request gets `503 VPN not connected`;
//!   destinations outside the tunnel's routes get `502`. Nothing ever goes direct.
//! - With a [`ProxyAuth`] (Android) every connection must authenticate first: SOCKS5 with
//!   username/password (first byte `0x05`, [`crate::socks`]) or an HTTP head with the right
//!   `Proxy-Authorization`; anything else is closed without an answer. Without one (Windows) the
//!   owner check decides and SOCKS is not spoken.
//!
//! The request head is parsed strictly and bounded: at most 16 KiB and 100 headers, CRLF line
//! ends only, no obs-fold, no whitespace before the colon, one `Content-Length` at most, no
//! `Content-Length` together with `Transfer-Encoding`, and `chunked` as the only transfer
//! coding. Request bodies are forwarded by their framing (chunked bodies are re-checked
//! chunk by chunk), so nothing after the body can slip through as a second request; a body
//! that stops moving for a minute ends with 408. `Expect: 100-continue` is answered by the
//! proxy itself (`100 Continue`, then the body) and not forwarded; other expectations get 417.

use crate::service::Shared;
use crate::socks::ProxyAuth;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Semaphore};

/// The largest request head.
pub const MAX_HEAD: usize = 16 * 1024;
/// The largest response head from a server.
pub const MAX_RESPONSE_HEAD: usize = 64 * 1024;
pub const MAX_HEADERS: usize = 100;
/// The most client connections at once.
pub const MAX_CONNECTIONS: usize = 512;
const HEAD_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
const IDLE_FORWARD: Duration = Duration::from_secs(300);
/// A request body that stops moving (client to server) for this long ends with 408.
const IDLE_BODY: Duration = Duration::from_secs(60);
const IDLE_TUNNEL: Duration = Duration::from_secs(1800);

/// A destination host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Host {
    Ip(IpAddr),
    Name(String),
}

impl Host {
    pub(crate) fn parse(s: &str) -> Option<Host> {
        if let Some(v6) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            return v6.parse::<std::net::Ipv6Addr>().ok().map(|a| Host::Ip(IpAddr::V6(a)));
        }
        if let Ok(ip) = s.parse::<std::net::Ipv4Addr>() {
            return Some(Host::Ip(IpAddr::V4(ip)));
        }
        crate::dns::normalize(s).map(Host::Name)
    }
}

impl std::fmt::Display for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Host::Ip(IpAddr::V6(a)) => write!(f, "[{a}]"),
            Host::Ip(a) => write!(f, "{a}"),
            Host::Name(n) => f.write_str(n),
        }
    }
}

/// What the client asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Connect {
        host: Host,
        port: u16,
    },
    /// An absolute-form `http://` request: `path` is the origin-form target.
    Forward {
        host: Host,
        port: u16,
        path: String,
    },
}

/// How the request body is framed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Body {
    None,
    Length(u64),
    Chunked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub target: Target,
    /// `HTTP/1.0` or `HTTP/1.1`.
    pub version: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

/// A request that is refused before anything is forwarded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reject {
    pub status: u16,
    pub reason: &'static str,
}

const fn reject(status: u16, reason: &'static str) -> Reject {
    Reject { status, reason }
}

fn is_tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// The end of the head (just after the empty line), if the buffer has it.
pub fn head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

/// Parses `host:port` (authority-form, port required).
fn authority(s: &str, default_port: Option<u16>) -> Option<(Host, u16)> {
    if s.is_empty() || s.contains('@') {
        return None;
    }
    let (host, port) = if s.starts_with('[') {
        let i = s.find(']')?;
        let rest = &s[i + 1..];
        let port = match rest.strip_prefix(':') {
            Some(p) => Some(p),
            None if rest.is_empty() => None,
            None => return None,
        };
        (&s[..=i], port)
    } else {
        match s.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (s, None),
        }
    };
    let port = match port {
        Some(p) => {
            if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            p.parse::<u16>().ok().filter(|&p| p != 0)?
        }
        None => default_port?,
    };
    Some((Host::parse(host)?, port))
}

/// Parses a request head (everything up to and including the empty line).
pub fn parse_request(head: &[u8]) -> Result<Request, Reject> {
    if head.len() > MAX_HEAD {
        return Err(reject(431, "Request Header Fields Too Large"));
    }
    let bad = reject(400, "Bad Request");
    let text = std::str::from_utf8(head).map_err(|_| bad)?;
    let text = text.strip_suffix("\r\n\r\n").ok_or(bad)?;
    // CR and LF only as CRLF pairs.
    if text.split("\r\n").any(|l| l.contains('\r') || l.contains('\n')) {
        return Err(bad);
    }
    let mut lines = text.split("\r\n");
    let line = lines.next().ok_or(bad)?;
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(bad);
    };
    if method.is_empty() || !method.bytes().all(is_tchar) {
        return Err(bad);
    }
    let version = match version {
        "HTTP/1.1" => "HTTP/1.1",
        "HTTP/1.0" => "HTTP/1.0",
        _ => return Err(reject(505, "HTTP Version Not Supported")),
    };
    if target.is_empty() || target.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err(bad);
    }

    let mut headers = Vec::new();
    for l in lines {
        if l.starts_with([' ', '\t']) {
            return Err(bad); // obs-fold
        }
        let (name, value) = l.split_once(':').ok_or(bad)?;
        if name.is_empty() || !name.bytes().all(is_tchar) {
            return Err(bad); // includes whitespace before the colon
        }
        let value = value.trim_matches([' ', '\t']);
        if value.bytes().any(|b| (b < b' ' && b != b'\t') || b == 0x7f) {
            return Err(bad);
        }
        headers.push((name.to_owned(), value.to_owned()));
        if headers.len() > MAX_HEADERS {
            return Err(reject(431, "Request Header Fields Too Large"));
        }
    }

    let count = |n: &str| headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case(n)).count();
    let get = |n: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.as_str());
    if count("host") > 1 || count("content-length") > 1 || count("transfer-encoding") > 1 {
        return Err(bad);
    }
    let body = match (get("content-length"), get("transfer-encoding")) {
        (Some(_), Some(_)) => return Err(bad),
        (Some(cl), None) => {
            if cl.is_empty() || cl.len() > 18 || !cl.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad);
            }
            match cl.parse::<u64>().map_err(|_| bad)? {
                0 => Body::None,
                n => Body::Length(n),
            }
        }
        (None, Some(te)) => {
            if version == "HTTP/1.0" {
                return Err(bad);
            }
            if !te.eq_ignore_ascii_case("chunked") {
                return Err(reject(501, "Not Implemented"));
            }
            Body::Chunked
        }
        (None, None) => Body::None,
    };

    let target = if method == "CONNECT" {
        if body != Body::None {
            return Err(bad);
        }
        let (host, port) = authority(target, None).ok_or(bad)?;
        Target::Connect { host, port }
    } else {
        let lower = target.get(..8).map(str::to_ascii_lowercase).unwrap_or_default();
        let rest = if lower.starts_with("http://") {
            &target[7..]
        } else if lower.starts_with("https://") {
            // Browsers use CONNECT for https; an absolute https URL would need TLS here.
            return Err(bad);
        } else {
            // Origin-form or `*`: not a proxy request.
            return Err(bad);
        };
        if rest.contains('#') {
            return Err(bad);
        }
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let (auth, path) = rest.split_at(end);
        let (host, port) = authority(auth, Some(80)).ok_or(bad)?;
        let path = if path.is_empty() {
            "/".to_owned()
        } else if path.starts_with('?') {
            format!("/{path}")
        } else {
            path.to_owned()
        };
        Target::Forward { host, port, path }
    };
    Ok(Request { method: method.to_owned(), target, version, headers, body })
}

/// Hop-by-hop headers (RFC 9110 §7.6.1) plus the proxy's own and `Host` (re-added).
fn hop_by_hop(headers: &[(String, String)]) -> Vec<String> {
    let mut names: Vec<String> = [
        "connection",
        "keep-alive",
        "proxy-connection",
        "proxy-authorization",
        "proxy-authenticate",
        "te",
        "trailer",
        "upgrade",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("connection") || k.eq_ignore_ascii_case("proxy-connection") {
            for t in v.split(',') {
                let t = t.trim().to_ascii_lowercase();
                // Never let Connection remove the framing headers.
                if !t.is_empty() && t != "content-length" && t != "transfer-encoding" && t != "host" {
                    names.push(t);
                }
            }
        }
    }
    names
}

/// The origin-form head sent to the server.
pub fn rewrite_request(req: &Request) -> Vec<u8> {
    let Target::Forward { host, port, path } = &req.target else { return Vec::new() };
    let drop = hop_by_hop(&req.headers);
    let mut out = format!("{} {} {}\r\n", req.method, path, req.version);
    if *port == 80 {
        out.push_str(&format!("Host: {host}\r\n"));
    } else {
        out.push_str(&format!("Host: {host}:{port}\r\n"));
    }
    for (k, v) in &req.headers {
        let lk = k.to_ascii_lowercase();
        // `Expect: 100-continue` is the proxy's to answer (see `forward`).
        if lk == "host" || lk == "expect" || drop.contains(&lk) {
            continue;
        }
        out.push_str(k);
        out.push_str(": ");
        out.push_str(v);
        out.push_str("\r\n");
    }
    out.push_str("Connection: close\r\n\r\n");
    out.into_bytes()
}

/// A response head from the server, rewritten for the client: hop-by-hop headers removed and
/// `Connection: close` added. Returns the new head and the status code.
pub fn rewrite_response(head: &[u8]) -> Option<(Vec<u8>, u16)> {
    let text = std::str::from_utf8(head).ok()?;
    let mut lines = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l));
    let status_line = lines.next()?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next()?;
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return None;
    }
    let code = parts.next()?;
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let status: u16 = code.parse().ok()?;
    let reason = parts.next().unwrap_or("");
    if reason.bytes().any(|b| b < b' ' && b != b'\t') {
        return None;
    }
    let mut headers = Vec::new();
    for l in lines {
        if l.is_empty() {
            continue;
        }
        if l.starts_with([' ', '\t']) {
            return None;
        }
        let (k, v) = l.split_once(':')?;
        if k.is_empty() || !k.bytes().all(is_tchar) {
            return None;
        }
        let v = v.trim_matches([' ', '\t']);
        if v.bytes().any(|b| (b < b' ' && b != b'\t') || b == 0x7f) {
            return None;
        }
        headers.push((k.to_owned(), v.to_owned()));
        if headers.len() > 4 * MAX_HEADERS {
            return None;
        }
    }
    let drop = hop_by_hop(&headers);
    let mut out = format!("{version} {status} {reason}\r\n");
    for (k, v) in &headers {
        if drop.contains(&k.to_ascii_lowercase()) {
            continue;
        }
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    if !(100..200).contains(&status) {
        out.push_str("Connection: close\r\n");
    }
    out.push_str("\r\n");
    Some((out.into_bytes(), status))
}

/// `Expect`: `Ok(true)` for `100-continue`, `Ok(false)` without the header, `Err` for any
/// other expectation (answered with 417).
pub fn expects_continue(req: &Request) -> Result<bool, Reject> {
    let mut any = false;
    for (k, v) in &req.headers {
        if k.eq_ignore_ascii_case("expect") {
            if !v.eq_ignore_ascii_case("100-continue") {
                return Err(reject(417, "Expectation Failed"));
            }
            any = true;
        }
    }
    Ok(any)
}

/// A complete short response (`Connection: close`, plain-text body).
pub fn simple_response(status: u16, reason: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// The fail-closed answer.
pub fn not_connected() -> Vec<u8> {
    simple_response(503, "VPN not connected", "VPN not connected\n")
}

// ---------------------------------------------------------------------------------------------
// The server.

/// Runs the accept loop until `stop` turns true.
pub(crate) async fn serve(
    listener: TcpListener,
    shared: Arc<Shared>,
    peer_allowed: Arc<dyn Fn(SocketAddr, SocketAddr) -> bool + Send + Sync>,
    auth: Option<Arc<ProxyAuth>>,
    mut stop: watch::Receiver<bool>,
) {
    let limit = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let accepted = tokio::select! {
            _ = stop.wait_for(|s| *s) => return,
            a = listener.accept() => a,
        };
        let Ok((stream, peer)) = accepted else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let Ok(permit) = limit.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let shared = shared.clone();
        let allowed = peer_allowed.clone();
        let auth = auth.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if !peer.ip().is_loopback() {
                return;
            }
            // The proxy's end of this connection (with the peer's: the TCP table's 4-tuple).
            let Ok(local) = stream.local_addr() else { return };
            let ok = tokio::task::spawn_blocking(move || allowed(local, peer)).await.unwrap_or(false);
            if !ok {
                return;
            }
            let _ = stream.set_nodelay(true);
            match auth {
                None => handle(stream, shared, None).await,
                Some(auth) => {
                    // SOCKS5 or HTTP, by the first byte (peeked: it stays in the stream).
                    let mut first = [0u8; 1];
                    match tokio::time::timeout(crate::socks::AUTH_TIMEOUT, stream.peek(&mut first)).await {
                        Ok(Ok(1)) if first[0] == 5 => crate::socks::serve_client(stream, shared, auth).await,
                        Ok(Ok(1)) => handle(stream, shared, Some(auth)).await,
                        _ => {}
                    }
                }
            }
        });
    }
}

async fn handle(mut client: TcpStream, shared: Arc<Shared>, auth: Option<Arc<ProxyAuth>>) {
    let mut buf = Vec::with_capacity(4096);
    // An unauthenticated peer gets little time (another app probing the port).
    let limit = if auth.is_some() { crate::socks::AUTH_TIMEOUT } else { HEAD_TIMEOUT };
    let end = match tokio::time::timeout(limit, read_head(&mut client, &mut buf, MAX_HEAD)).await {
        Ok(Ok(Some(end))) => end,
        Ok(Ok(None)) if auth.is_none() => {
            let _ = client
                .write_all(&simple_response(431, "Request Header Fields Too Large", "Request header too large\n"))
                .await;
            return;
        }
        _ => return,
    };
    let req = match parse_request(&buf[..end]) {
        Ok(r) => r,
        // With a credential, nothing is answered before it was checked.
        Err(_) if auth.is_some() => return,
        Err(r) => {
            let _ = client.write_all(&simple_response(r.status, r.reason, &format!("{}\n", r.reason))).await;
            return;
        }
    };
    if let Some(auth) = &auth {
        if !auth.check_headers(&req.headers) {
            return;
        }
    }
    let leftover = buf.split_off(end);
    if !shared.is_connected() {
        let _ = client.write_all(&not_connected()).await;
        return;
    }
    let (host, port) = match &req.target {
        Target::Connect { host, port } | Target::Forward { host, port, .. } => (host.clone(), *port),
    };
    let opened = tokio::select! {
        r = open(&shared, &host, port) => r,
        _ = shared.disconnected() => Err(OpenFail::NotConnected),
    };
    let upstream = match opened {
        Ok(s) => s,
        Err(fail) => {
            if fail.is_refusal() {
                shared.note_refused(&host.to_string());
            }
            let (status, reason, text) = fail.http();
            let _ = client.write_all(&simple_response(status, reason, text)).await;
            return;
        }
    };
    let piped = async {
        match req.target {
            Target::Connect { .. } => tunnel(&mut client, upstream, leftover).await,
            Target::Forward { .. } => forward(&mut client, upstream, &req, leftover, IDLE_BODY).await,
        }
    };
    let cut = if auth.is_some() {
        // Android: when the tunnel goes down, a connection piped through it is reset at once
        // instead of hanging until the engine's own timeout (a pooled keep-alive connection
        // otherwise kept a page loading for about a minute, a9 T5). The engine then retries on
        // a new connection and gets the fail-closed refusal. Windows keeps its 0.14 behaviour.
        tokio::select! {
            cut = piped => cut,
            _ = shared.disconnected() => true,
        }
    } else {
        piped.await
    };
    if cut {
        abort(&client);
    }
}

/// The server's side broke off (reset, or the tunnel went away): the client gets a reset too
/// (`SO_LINGER` 0, then the close), never a clean end that would pass for a complete response.
pub(crate) fn abort(client: &TcpStream) {
    let _ = socket2::SockRef::from(client).set_linger(Some(Duration::ZERO));
}

/// Reads until the end of the head; `None` when it exceeds `max`. Reads in small steps so the
/// bytes after the head stay in `buf` for the body.
async fn read_head<R: AsyncRead + Unpin>(r: &mut R, buf: &mut Vec<u8>, max: usize) -> io::Result<Option<usize>> {
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(e) = head_end(buf) {
            return Ok(Some(e));
        }
        if buf.len() > max {
            return Ok(None);
        }
        let n = r.read(&mut chunk).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Why a connection through the tunnel could not be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenFail {
    NotConnected,
    DnsTimeout,
    NameNotFound,
    /// Outside the profile's routes.
    NotRouted,
    TimedOut,
    Refused,
}

impl OpenFail {
    /// The HTTP answer: status, reason, body.
    pub(crate) fn http(self) -> (u16, &'static str, &'static str) {
        match self {
            OpenFail::NotConnected => (503, "VPN not connected", "VPN not connected\n"),
            OpenFail::DnsTimeout => (504, "Gateway Timeout", "The VPN's DNS server did not answer\n"),
            OpenFail::NameNotFound => (502, "Bad Gateway", "The name was not found through the VPN\n"),
            OpenFail::NotRouted => (502, "Bad Gateway", "This address is not reachable through the VPN\n"),
            OpenFail::TimedOut => (504, "Gateway Timeout", "The connection through the VPN timed out\n"),
            OpenFail::Refused => (502, "Bad Gateway", "The connection through the VPN was refused\n"),
        }
    }

    /// The destination is not reachable through this VPN at all (shown as "Not reachable
    /// through this VPN" by the Android error page, see `Vpn::refused_recently`).
    pub(crate) fn is_refusal(self) -> bool {
        matches!(self, OpenFail::NameNotFound | OpenFail::NotRouted)
    }
}

/// Resolves inside the tunnel, checks the routes, connects.
pub(crate) async fn open(shared: &Shared, host: &Host, port: u16) -> Result<crate::net::TcpStream, OpenFail> {
    let addrs = match host {
        Host::Ip(ip) => vec![*ip],
        Host::Name(n) => match shared.dns.resolve(&shared.net, n).await {
            Ok(a) => a,
            Err(crate::dns::DnsError::Timeout) => return Err(OpenFail::DnsTimeout),
            Err(_) => return Err(OpenFail::NameNotFound),
        },
    };
    let usable: Vec<IpAddr> = addrs
        .into_iter()
        .filter(|ip| !(ip.is_loopback() || ip.is_unspecified() || ip.is_multicast()))
        .filter(|ip| shared.routes_allow(*ip))
        .take(3)
        .collect();
    if usable.is_empty() {
        return Err(OpenFail::NotRouted);
    }
    let mut timed_out = false;
    for ip in usable {
        if !shared.is_connected() {
            return Err(OpenFail::NotConnected);
        }
        match shared.net.tcp_connect(SocketAddr::new(ip, port), CONNECT_TIMEOUT).await {
            Ok(s) => return Ok(s),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => timed_out = true,
            Err(_) => {}
        }
    }
    if timed_out {
        Err(OpenFail::TimedOut)
    } else {
        Err(OpenFail::Refused)
    }
}

/// A CONNECT-like tunnel that is already answered (SOCKS5): both ways until done. True when
/// the server's side broke off.
pub(crate) async fn pipe_tunnel(client: &mut TcpStream, upstream: crate::net::TcpStream) -> bool {
    pipe(client, upstream, IDLE_TUNNEL).await
}

/// CONNECT: both ways until done. True when the server's side broke off.
async fn tunnel(client: &mut TcpStream, mut upstream: crate::net::TcpStream, leftover: Vec<u8>) -> bool {
    if client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await.is_err() {
        return false;
    }
    if !leftover.is_empty() && upstream.write_all(&leftover).await.is_err() {
        return true;
    }
    pipe(client, upstream, IDLE_TUNNEL).await
}

/// One absolute-form request: the head and the body to the server, then its response back
/// until it closes. `body_idle` limits how long the request body may stand still. True when
/// the response was cut off by the server's side breaking off (the client must see an error).
async fn forward<C, U>(client: &mut C, mut upstream: U, req: &Request, leftover: Vec<u8>, body_idle: Duration) -> bool
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    let expect = match expects_continue(req) {
        Ok(e) => e && req.body != Body::None,
        Err(r) => {
            let _ = client.write_all(&simple_response(r.status, r.reason, &format!("{}\n", r.reason))).await;
            return false;
        }
    };
    let head = rewrite_request(req);
    if upstream.write_all(&head).await.is_err() {
        return false;
    }
    // The client waits for this before it sends the body; the server never saw the Expect.
    if expect && req.version == "HTTP/1.1" && client.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await.is_err() {
        return false;
    }
    let mut body = BodyReader { buf: leftover, pos: 0 };
    let sent = {
        let activity = Activity::new();
        let mut from = Tracked { inner: &mut *client, activity: activity.clone() };
        let mut to = Tracked { inner: &mut upstream, activity: activity.clone() };
        let copy = async {
            match req.body {
                Body::None => Ok(()),
                Body::Length(n) => body.copy_exact(&mut from, &mut to, n).await,
                Body::Chunked => body.copy_chunked(&mut from, &mut to).await,
            }
        };
        tokio::select! {
            r = copy => r,
            _ = activity.idle(body_idle) => Err(io::ErrorKind::TimedOut.into()),
        }
    };
    match sent {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::TimedOut => {
            let _ = client.write_all(&simple_response(408, "Request Timeout", "The request body stopped\n")).await;
            return false;
        }
        Err(_) => {
            let _ = client.write_all(&simple_response(400, "Bad Request", "Bad request body\n")).await;
            return false;
        }
    }
    // The response head(s): 1xx heads are passed on, then the final one.
    let mut rbuf = Vec::with_capacity(4096);
    loop {
        let end = match tokio::time::timeout(RESPONSE_TIMEOUT, read_response_head(&mut upstream, &mut rbuf)).await {
            Ok(Ok(Some(e))) => e,
            Ok(_) => {
                let _ =
                    client.write_all(&simple_response(502, "Bad Gateway", "The server sent a bad response\n")).await;
                return false;
            }
            Err(_) => {
                let _ = client.write_all(&simple_response(504, "Gateway Timeout", "The server did not answer\n")).await;
                return false;
            }
        };
        let rest = rbuf.split_off(end);
        let Some((head, status)) = rewrite_response(&rbuf) else {
            let _ = client.write_all(&simple_response(502, "Bad Gateway", "The server sent a bad response\n")).await;
            return false;
        };
        if status == 101 {
            let _ = client.write_all(&simple_response(502, "Bad Gateway", "Protocol upgrades are not proxied\n")).await;
            return false;
        }
        if client.write_all(&head).await.is_err() {
            return false;
        }
        rbuf = rest;
        if !(100..200).contains(&status) {
            break;
        }
    }
    if !rbuf.is_empty() && client.write_all(&rbuf).await.is_err() {
        return false;
    }
    // The rest of the response until the server closes; the client's side is done.
    let (mut cr, mut cw) = tokio::io::split(client);
    let activity = Activity::new();
    let mut up = Tracked { inner: upstream, activity: activity.clone() };
    let copy = copy_reporting(&mut up, &mut cw);
    let drain = async {
        // Read (and discard) anything more the client sends, to notice when it goes away.
        let mut sink = [0u8; 1024];
        while let Ok(n) = cr.read(&mut sink).await {
            if n == 0 {
                std::future::pending::<()>().await;
            }
        }
    };
    let cut = tokio::select! {
        r = copy => r == Copied::ReadFailed,
        _ = drain => false,
        _ = activity.idle(IDLE_FORWARD) => false,
    };
    if !cut {
        let _ = cw.shutdown().await;
    }
    cut
}

#[derive(PartialEq, Eq)]
enum Copied {
    /// The reader ended cleanly.
    Done,
    /// Reading failed (a reset, the tunnel went away).
    ReadFailed,
    /// Writing failed (the client went away).
    WriteFailed,
}

/// `tokio::io::copy` that says which side failed.
async fn copy_reporting<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(r: &mut R, w: &mut W) -> Copied {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = match r.read(&mut buf).await {
            Ok(0) => return Copied::Done,
            Ok(n) => n,
            Err(_) => return Copied::ReadFailed,
        };
        if w.write_all(&buf[..n]).await.is_err() {
            return Copied::WriteFailed;
        }
    }
}

/// Reads a response head (CRLF or bare LF line ends); `None` when too large or not HTTP.
async fn read_response_head<R: AsyncRead + Unpin>(r: &mut R, buf: &mut Vec<u8>) -> io::Result<Option<usize>> {
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(i) = buf.windows(2).position(|w| w == b"\n\n") {
            return Ok(Some(i + 2));
        }
        if let Some(e) = head_end(buf) {
            return Ok(Some(e));
        }
        if buf.len() >= 5 && !buf.starts_with(b"HTTP/") {
            return Ok(None);
        }
        if buf.len() > MAX_RESPONSE_HEAD {
            return Ok(None);
        }
        let n = r.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// The client's request body: what came with the head first, then the socket.
struct BodyReader {
    buf: Vec<u8>,
    pos: usize,
}

impl BodyReader {
    async fn fill<R: AsyncRead + Unpin>(&mut self, r: &mut R) -> io::Result<()> {
        if self.pos >= self.buf.len() {
            self.buf.clear();
            self.pos = 0;
            self.buf.resize(16384, 0);
            let n = r.read(&mut self.buf).await?;
            self.buf.truncate(n);
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
        Ok(())
    }

    async fn copy_exact<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
        &mut self,
        r: &mut R,
        w: &mut W,
        mut n: u64,
    ) -> io::Result<()> {
        while n > 0 {
            self.fill(r).await?;
            let take = usize::try_from(n).unwrap_or(usize::MAX).min(self.buf.len() - self.pos);
            w.write_all(&self.buf[self.pos..self.pos + take]).await?;
            self.pos += take;
            n -= take as u64;
        }
        Ok(())
    }

    /// One CRLF-terminated line (at most 4 KiB), returned without the CRLF.
    async fn line<R: AsyncRead + Unpin>(&mut self, r: &mut R) -> io::Result<Vec<u8>> {
        let mut line = Vec::new();
        loop {
            self.fill(r).await?;
            let avail = &self.buf[self.pos..];
            if let Some(i) = avail.iter().position(|&b| b == b'\n') {
                line.extend_from_slice(&avail[..i]);
                self.pos += i + 1;
                break;
            }
            line.extend_from_slice(avail);
            self.pos = self.buf.len();
            if line.len() > 4096 {
                return Err(io::ErrorKind::InvalidData.into());
            }
        }
        if line.pop() != Some(b'\r') || line.contains(&b'\r') || line.len() > 4096 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(line)
    }

    async fn copy_chunked<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
        &mut self,
        r: &mut R,
        w: &mut W,
    ) -> io::Result<()> {
        loop {
            let line = self.line(r).await?;
            let size = chunk_size(&line).ok_or(io::ErrorKind::InvalidData)?;
            w.write_all(&line).await?;
            w.write_all(b"\r\n").await?;
            if size == 0 {
                break;
            }
            self.copy_exact(r, w, size).await?;
            if !self.line(r).await?.is_empty() {
                return Err(io::ErrorKind::InvalidData.into());
            }
            w.write_all(b"\r\n").await?;
        }
        // Trailers, up to the empty line.
        let mut total = 0;
        loop {
            let line = self.line(r).await?;
            total += line.len();
            if total > MAX_HEAD {
                return Err(io::ErrorKind::InvalidData.into());
            }
            if !line.is_empty() {
                let ok = line
                    .iter()
                    .position(|&b| b == b':')
                    .is_some_and(|i| i > 0 && line[..i].iter().all(|&b| is_tchar(b)));
                if !ok {
                    return Err(io::ErrorKind::InvalidData.into());
                }
            }
            w.write_all(&line).await?;
            w.write_all(b"\r\n").await?;
            if line.is_empty() {
                return Ok(());
            }
        }
    }
}

/// The size of a chunk-size line (`1a;ext=1`): hex, at most 15 digits.
pub fn chunk_size(line: &[u8]) -> Option<u64> {
    let hex = match line.iter().position(|&b| b == b';') {
        Some(i) => &line[..i],
        None => line,
    };
    let hex = std::str::from_utf8(hex).ok()?.trim_end_matches([' ', '\t']);
    if hex.is_empty() || hex.len() > 15 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if line.iter().any(|&b| b < b' ' && b != b'\t') {
        return None;
    }
    u64::from_str_radix(hex, 16).ok()
}

/// Both ways until both sides are done, or nothing moves for `idle`. True when the upstream
/// side broke off (reset, or the tunnel went away).
async fn pipe(client: &mut TcpStream, upstream: crate::net::TcpStream, idle: Duration) -> bool {
    let activity = Activity::new();
    let (mut cr, mut cw) = client.split();
    let (mut ur, mut uw) = tokio::io::split(upstream);
    let mut cr = Tracked { inner: &mut cr, activity: activity.clone() };
    let mut ur = Tracked { inner: &mut ur, activity: activity.clone() };
    let up = async {
        let r = copy_reporting(&mut cr, &mut uw).await;
        if r == Copied::Done {
            let _ = uw.shutdown().await;
        }
        r
    };
    let down = async {
        let r = copy_reporting(&mut ur, &mut cw).await;
        if r == Copied::Done {
            let _ = cw.shutdown().await;
        }
        r
    };
    tokio::pin!(up, down);
    let (mut up_done, mut down_done) = (false, false);
    loop {
        tokio::select! {
            r = &mut up, if !up_done => match r {
                Copied::Done => up_done = true,
                // Writing to the server failed: it broke off. Reading the client failed: the
                // client went away (nothing to tell it).
                r => return r == Copied::WriteFailed,
            },
            r = &mut down, if !down_done => match r {
                Copied::Done => down_done = true,
                r => return r == Copied::ReadFailed,
            },
            _ = activity.idle(idle) => return false,
        }
        if up_done && down_done {
            return false;
        }
    }
}

/// The last time a tracked stream moved data.
#[derive(Clone)]
struct Activity {
    start: Instant,
    last_ms: Arc<AtomicU64>,
}

impl Activity {
    fn new() -> Activity {
        Activity { start: Instant::now(), last_ms: Arc::new(AtomicU64::new(0)) }
    }
    fn touch(&self) {
        self.last_ms.store(self.start.elapsed().as_millis() as u64, Ordering::Relaxed);
    }
    async fn idle(&self, limit: Duration) {
        loop {
            let last = Duration::from_millis(self.last_ms.load(Ordering::Relaxed));
            let since = self.start.elapsed().saturating_sub(last);
            if since >= limit {
                return;
            }
            tokio::time::sleep(limit - since + Duration::from_millis(10)).await;
        }
    }
}

struct Tracked<S> {
    inner: S,
    activity: Activity,
}

impl<S: AsyncRead + Unpin> AsyncRead for Tracked<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let r = Pin::new(&mut self.inner).poll_read(cx, buf);
        if buf.filled().len() > before {
            self.activity.touch();
        }
        r
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Tracked<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let r = Pin::new(&mut self.inner).poll_write(cx, data);
        if matches!(r, Poll::Ready(Ok(n)) if n > 0) {
            self.activity.touch();
        }
        r
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Request, Reject> {
        parse_request(s.as_bytes())
    }

    #[test]
    fn connect() {
        let r = p("CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n").unwrap();
        assert_eq!(r.target, Target::Connect { host: Host::Name("example.com".into()), port: 443 });
        let r = p("CONNECT [2001:db8::1]:8443 HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(r.target, Target::Connect { host: Host::Ip("2001:db8::1".parse().unwrap()), port: 8443 });
        let r = p("CONNECT 10.77.0.10:80 HTTP/1.0\r\n\r\n").unwrap();
        assert_eq!(r.target, Target::Connect { host: Host::Ip("10.77.0.10".parse().unwrap()), port: 80 });
        for bad in [
            "CONNECT example.com HTTP/1.1\r\n\r\n",
            "CONNECT example.com:0 HTTP/1.1\r\n\r\n",
            "CONNECT example.com:99999 HTTP/1.1\r\n\r\n",
            "CONNECT user@example.com:443 HTTP/1.1\r\n\r\n",
            "CONNECT [::1:443 HTTP/1.1\r\n\r\n",
            "CONNECT exa mple.com:443 HTTP/1.1\r\n\r\n",
            "CONNECT example.com:443 HTTP/1.1\r\nContent-Length: 5\r\n\r\n",
            "CONNECT ex%61mple.com:443 HTTP/1.1\r\n\r\n",
        ] {
            assert_eq!(p(bad).unwrap_err().status, 400, "{bad:?}");
        }
    }

    #[test]
    fn absolute_form() {
        let r = p("GET http://Inside.Test/a/b?c=1 HTTP/1.1\r\nHost: wrong\r\nAccept: */*\r\nProxy-Connection: keep-alive\r\n\
                   Connection: keep-alive, X-Private\r\nX-Private: 1\r\nKeep-Alive: 300\r\nProxy-Authorization: Basic eA==\r\n\r\n")
        .unwrap();
        assert_eq!(
            r.target,
            Target::Forward { host: Host::Name("inside.test".into()), port: 80, path: "/a/b?c=1".into() }
        );
        let out = String::from_utf8(rewrite_request(&r)).unwrap();
        assert_eq!(out, "GET /a/b?c=1 HTTP/1.1\r\nHost: inside.test\r\nAccept: */*\r\nConnection: close\r\n\r\n");
        let r = p("POST HTTP://10.0.0.1:8080?x HTTP/1.1\r\nContent-Length: 3\r\n\r\n").unwrap();
        assert_eq!(r.body, Body::Length(3));
        assert_eq!(
            r.target,
            Target::Forward { host: Host::Ip("10.0.0.1".parse().unwrap()), port: 8080, path: "/?x".into() }
        );
        let out = String::from_utf8(rewrite_request(&r)).unwrap();
        assert!(out.starts_with("POST /?x HTTP/1.1\r\nHost: 10.0.0.1:8080\r\nContent-Length: 3\r\n"), "{out}");
        // Connection cannot drop the framing headers.
        let r = p("POST http://a.test/ HTTP/1.1\r\nConnection: content-length\r\nContent-Length: 2\r\n\r\n").unwrap();
        assert!(String::from_utf8(rewrite_request(&r)).unwrap().contains("Content-Length: 2"));
        let r = p("PUT http://a.test/ HTTP/1.1\r\nTransfer-Encoding: Chunked\r\n\r\n").unwrap();
        assert_eq!(r.body, Body::Chunked);
        for bad in [
            "GET /path HTTP/1.1\r\nHost: a\r\n\r\n",
            "GET https://a.test/ HTTP/1.1\r\n\r\n",
            "GET http://user:pw@a.test/ HTTP/1.1\r\n\r\n",
            "GET http:///x HTTP/1.1\r\n\r\n",
            "GET http://a.test/#frag HTTP/1.1\r\n\r\n",
            "OPTIONS * HTTP/1.1\r\n\r\n",
            "GET  http://a.test/ HTTP/1.1\r\n\r\n",
            "GET http://a.test/ HTTP/1.1 \r\n\r\n",
            "G(T http://a.test/ HTTP/1.1\r\n\r\n",
        ] {
            assert_eq!(p(bad).unwrap_err().status, 400, "{bad:?}");
        }
        assert_eq!(p("GET http://a.test/ HTTP/2.0\r\n\r\n").unwrap_err().status, 505);
    }

    #[test]
    fn smuggling_and_limits() {
        let base = "POST http://a.test/ HTTP/1.1\r\n";
        for (hdrs, status) in [
            ("Content-Length: 5\r\nContent-Length: 5\r\n", 400),
            ("Content-Length: 5\r\nContent-Length: 6\r\n", 400),
            ("Content-Length: 5, 5\r\n", 400),
            ("Content-Length: +5\r\n", 400),
            ("Content-Length: 5\r\nTransfer-Encoding: chunked\r\n", 400),
            ("Transfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n", 400),
            ("Transfer-Encoding: gzip, chunked\r\n", 501),
            ("Transfer-Encoding: xchunked\r\n", 501),
            ("Transfer-Encoding : chunked\r\n", 400),
            ("Content-Length : 5\r\n", 400),
            ("X-A: 1\r\n folded\r\n", 400),
            ("X-A: 1\nContent-Length: 5\r\n", 400),
            ("X-A: a\rb\r\n", 400),
            ("Host: a\r\nHost: b\r\n", 400),
            ("X-A: \u{1}\r\n", 400),
            ("NoColon\r\n", 400),
            (": empty-name\r\n", 400),
        ] {
            let req = format!("{base}{hdrs}\r\n");
            assert_eq!(p(&req).unwrap_err().status, status, "{hdrs:?}");
        }
        let te10 = "POST http://a.test/ HTTP/1.0\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(p(te10).unwrap_err().status, 400);
        let many = format!("{base}{}\r\n", "X-A: 1\r\n".repeat(MAX_HEADERS + 1));
        assert_eq!(p(&many).unwrap_err().status, 431);
        let huge = format!("{base}X-A: {}\r\n\r\n", "a".repeat(MAX_HEAD));
        assert_eq!(p(&huge).unwrap_err().status, 431);
        assert_eq!(parse_request(b"GET http://a.test/ HTTP/1.1\r\nX: \xff\r\n\r\n").unwrap_err().status, 400);
        assert_eq!(p("GET http://a.test/ HTTP/1.1\r\n").unwrap_err().status, 400);
    }

    #[test]
    fn chunk_sizes() {
        assert_eq!(chunk_size(b"1a"), Some(26));
        assert_eq!(chunk_size(b"0"), Some(0));
        assert_eq!(chunk_size(b"10;name=value"), Some(16));
        assert_eq!(chunk_size(b""), None);
        assert_eq!(chunk_size(b"-1"), None);
        assert_eq!(chunk_size(b"0x10"), None);
        assert_eq!(chunk_size(b"ffffffffffffffff"), None);
        assert_eq!(chunk_size(b"1 2"), None);
    }

    #[tokio::test]
    async fn chunked_body_is_rechecked() {
        async fn run(input: &[u8]) -> io::Result<Vec<u8>> {
            let mut r = BodyReader { buf: input[..3.min(input.len())].to_vec(), pos: 0 };
            let mut rest = &input[3.min(input.len())..];
            let mut out = Vec::new();
            r.copy_chunked(&mut rest, &mut out).await?;
            Ok(out)
        }
        let good = b"4\r\nWiki\r\n5;x=y\r\npedia\r\n0\r\nX-T: 1\r\n\r\nGET http://evil/ HTTP/1.1\r\n\r\n";
        let out = run(good).await.unwrap();
        assert_eq!(out, b"4\r\nWiki\r\n5;x=y\r\npedia\r\n0\r\nX-T: 1\r\n\r\n");
        assert!(run(b"4\r\nWikiXX\r\n0\r\n\r\n").await.is_err());
        assert!(run(b"4\nWiki\r\n0\r\n\r\n").await.is_err());
        assert!(run(b"zz\r\n").await.is_err());
        assert!(run(b"0\r\nbad trailer\r\n\r\n").await.is_err());
        assert!(run(b"4\r\nWi").await.is_err());
        let mut r = BodyReader { buf: b"abcdefgh".to_vec(), pos: 0 };
        let mut out = Vec::new();
        r.copy_exact(&mut &b""[..], &mut out, 5).await.unwrap();
        assert_eq!(out, b"abcde");
    }

    #[test]
    fn responses() {
        let (h, s) = rewrite_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive, X-Hop\r\nX-Hop: 1\r\nKeep-Alive: timeout=5\r\nSet-Cookie: a=b\r\n\r\n",
        )
        .unwrap();
        assert_eq!(s, 200);
        assert_eq!(
            String::from_utf8(h).unwrap(),
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nSet-Cookie: a=b\r\nConnection: close\r\n\r\n"
        );
        let (h, s) = rewrite_response(b"HTTP/1.0 404 Not Found\n\n").unwrap();
        assert_eq!(
            (String::from_utf8(h).unwrap().as_str(), s),
            ("HTTP/1.0 404 Not Found\r\nConnection: close\r\n\r\n", 404)
        );
        let (h, _) = rewrite_response(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
        assert_eq!(h, b"HTTP/1.1 100 Continue\r\n\r\n");
        assert!(rewrite_response(b"HTTP/1.1 20 OK\r\n\r\n").is_none());
        assert!(rewrite_response(b"ICY 200 OK\r\n\r\n").is_none());
        assert!(rewrite_response(b"HTTP/1.1 200 OK\r\nX: 1\r\n folded\r\n\r\n").is_none());
        assert!(rewrite_response(b"HTTP/1.1 200 OK\r\nBad Name: 1\r\n\r\n").is_none());
    }

    /// The proxy credential is never forwarded to the server (absolute-form requests; CONNECT
    /// forwards nothing of the head).
    #[test]
    fn credential_is_not_forwarded() {
        let r = p("GET http://example.com/x HTTP/1.1\r\nProxy-Authorization: Basic a3J1bms6c2VjcmV0\r\n\
                   proxy-authorization: Basic eA==\r\nAccept: */*\r\n\r\n")
        .unwrap();
        let out = String::from_utf8(rewrite_request(&r)).unwrap();
        assert!(!out.to_ascii_lowercase().contains("proxy-authorization"), "{out}");
        assert!(!out.contains("a3J1bms6c2VjcmV0") && !out.contains("eA=="), "{out}");
        assert!(out.contains("Accept: */*\r\n"));
    }

    #[test]
    fn fail_closed_body() {
        let r = String::from_utf8(not_connected()).unwrap();
        assert!(r.starts_with("HTTP/1.1 503 VPN not connected\r\n"), "{r}");
        assert!(r.ends_with("\r\n\r\nVPN not connected\n"));
        assert!(r.contains("Content-Length: 18\r\n"));
    }

    /// Reads from `r` until `needle` has arrived (or EOF); everything read.
    async fn read_until<R: AsyncRead + Unpin>(r: &mut R, needle: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut b = [0u8; 512];
        while !out.windows(needle.len()).any(|w| w == needle) {
            match r.read(&mut b).await {
                Ok(0) | Err(_) => break,
                Ok(n) => out.extend_from_slice(&b[..n]),
            }
        }
        out
    }

    #[tokio::test]
    async fn expect_100_continue_is_answered_without_a_deadlock() {
        let req = p("POST http://a.test/up HTTP/1.1\r\nExpect: 100-Continue\r\nContent-Length: 4\r\n\r\n").unwrap();
        assert_eq!(expects_continue(&req), Ok(true));
        let (mut client, mut proxy_side) = tokio::io::duplex(4096);
        let (proxy_up, mut server) = tokio::io::duplex(4096);
        let task =
            tokio::spawn(
                async move { forward(&mut proxy_side, proxy_up, &req, Vec::new(), Duration::from_secs(5)).await },
            );
        // The client waits for the 100 before it sends the body, like curl or .NET do.
        let got = tokio::time::timeout(Duration::from_secs(5), read_until(&mut client, b"\r\n\r\n")).await.unwrap();
        assert_eq!(got, b"HTTP/1.1 100 Continue\r\n\r\n");
        client.write_all(b"body").await.unwrap();
        let head = tokio::time::timeout(Duration::from_secs(5), read_until(&mut server, b"body")).await.unwrap();
        let head = String::from_utf8(head).unwrap();
        assert!(head.starts_with("POST /up HTTP/1.1\r\nHost: a.test\r\n"), "{head}");
        assert!(!head.to_ascii_lowercase().contains("expect"), "{head}");
        server.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\nok").await.unwrap();
        drop(server);
        let rest = tokio::time::timeout(Duration::from_secs(5), read_until(&mut client, b"ok")).await.unwrap();
        assert!(String::from_utf8(rest).unwrap().starts_with("HTTP/1.1 201 Created\r\n"));
        assert!(!task.await.unwrap(), "a clean end");

        // Any other expectation: 417, nothing forwarded.
        let req = p("POST http://a.test/ HTTP/1.1\r\nExpect: magic\r\nContent-Length: 4\r\n\r\n").unwrap();
        assert_eq!(expects_continue(&req).unwrap_err().status, 417);
        let (mut client, mut proxy_side) = tokio::io::duplex(4096);
        let (proxy_up, mut server) = tokio::io::duplex(4096);
        assert!(!forward(&mut proxy_side, proxy_up, &req, Vec::new(), Duration::from_secs(5)).await);
        drop(proxy_side);
        let mut r = Vec::new();
        client.read_to_end(&mut r).await.unwrap();
        assert!(r.starts_with(b"HTTP/1.1 417 Expectation Failed\r\n"));
        let mut up = Vec::new();
        server.read_to_end(&mut up).await.unwrap();
        assert!(up.is_empty());
    }

    #[tokio::test]
    async fn a_stalled_request_body_times_out() {
        let req = p("PUT http://a.test/ HTTP/1.1\r\nContent-Length: 10\r\n\r\n").unwrap();
        let (mut client, mut proxy_side) = tokio::io::duplex(4096);
        let (proxy_up, _server) = tokio::io::duplex(4096);
        client.write_all(b"abc").await.unwrap();
        let t = Instant::now();
        let task = tokio::spawn(async move {
            forward(&mut proxy_side, proxy_up, &req, Vec::new(), Duration::from_millis(300)).await
        });
        let got = tokio::time::timeout(Duration::from_secs(5), read_until(&mut client, b"stopped\n")).await.unwrap();
        assert!(got.starts_with(b"HTTP/1.1 408 Request Timeout\r\n"), "{:?}", String::from_utf8_lossy(&got));
        assert!(t.elapsed() >= Duration::from_millis(300));
        assert!(!task.await.unwrap());
    }

    #[tokio::test]
    async fn a_response_cut_off_by_the_server_is_reported() {
        /// An upstream that sends a head and some body, then fails like a reset.
        struct Cut(Vec<u8>);
        impl AsyncRead for Cut {
            fn poll_read(mut self: Pin<&mut Self>, _: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
                if self.0.is_empty() {
                    return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
                }
                let n = self.0.len().min(buf.remaining());
                buf.put_slice(&self.0[..n]);
                self.0.drain(..n);
                Poll::Ready(Ok(()))
            }
        }
        impl AsyncWrite for Cut {
            fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, d: &[u8]) -> Poll<io::Result<usize>> {
                Poll::Ready(Ok(d.len()))
            }
            fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
                Poll::Ready(Ok(()))
            }
            fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
                Poll::Ready(Ok(()))
            }
        }
        let req = p("GET http://a.test/ HTTP/1.1\r\n\r\n").unwrap();
        let (_client, mut proxy_side) = tokio::io::duplex(65536);
        let up = Cut(b"HTTP/1.1 200 OK\r\n\r\npart of the bo".to_vec());
        assert!(forward(&mut proxy_side, up, &req, Vec::new(), Duration::from_secs(5)).await);
    }

    #[test]
    fn head_ends() {
        assert_eq!(head_end(b"GET / HTTP/1.1\r\n\r\nbody"), Some(18));
        assert_eq!(head_end(b"GET / HTTP/1.1\r\n"), None);
    }
}
