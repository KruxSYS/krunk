//! The proxy credential and the SOCKS5 front (Android).
//!
//! On Windows the proxy lets in only connections whose owner is Krunk (the owner-PID check). Android
//! has no such check: any app can connect to `127.0.0.1:<port>`. There the VPN is started with a
//! per-session random token ([`ProxyAuth`]) and every connection must prove it knows it:
//!
//! - **SOCKS5** (RFC 1928, username/password RFC 1929): the engine's channel filter. GeckoView
//!   does not send a `Proxy-Authorization` header on CONNECT for an `http` proxy and answers a 407
//!   with a prompt to the user (Android 0.10.0 spike S2), but it sends SOCKS5 credentials. User
//!   `krunk`, password the token as 64 lowercase hex characters. Only CONNECT; host names are
//!   resolved inside the tunnel like everything else.
//! - **HTTP** with `Proxy-Authorization: Basic base64("krunk:" + hex)`: accepted for clients that
//!   can set the header; Krunk for Android uses SOCKS5 for its own requests as well.
//!
//! A connection that does not authenticate is closed without any reply (no 407, no SOCKS
//! "no acceptable method" or RFC 1929 failure byte), within [`AUTH_TIMEOUT`]. The only byte a
//! prober sees is the method choice (`05 02`) that RFC 1929 needs before the credential. The comparison runs
//! in constant time. The token never leaves this process's memory and is zeroized on drop.

use crate::proxy::{Host, OpenFail};
use crate::service::Shared;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zeroize::Zeroizing;

/// The user name in both forms of the credential.
pub const USER: &str = "krunk";
/// How long an unauthenticated peer may take for its greeting, credential or request head.
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
/// How long an authenticated SOCKS client may take for its request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The per-session credential. Built from 32 random bytes; holds only the forms it compares.
pub struct ProxyAuth {
    /// The token as 64 lowercase hex characters (the SOCKS password).
    hex: Zeroizing<String>,
    /// `Basic base64("krunk:" + hex)`, the HTTP header value.
    basic: Zeroizing<String>,
    ok: AtomicU64,
    refused: AtomicU64,
}

impl ProxyAuth {
    pub fn new(token: &[u8; 32]) -> ProxyAuth {
        use base64ct::Encoding;
        let mut hex = Zeroizing::new(String::with_capacity(64));
        for b in token {
            hex.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
            hex.push(char::from_digit(u32::from(b & 15), 16).unwrap_or('0'));
        }
        let plain = Zeroizing::new(format!("{USER}:{}", hex.as_str()));
        let basic = Zeroizing::new(format!("Basic {}", base64ct::Base64::encode_string(plain.as_bytes())));
        ProxyAuth { hex, basic, ok: AtomicU64::new(0), refused: AtomicU64::new(0) }
    }

    /// From the token's bytes; `None` unless exactly 32.
    pub fn from_slice(token: &[u8]) -> Option<ProxyAuth> {
        let t: &[u8; 32] = token.try_into().ok()?;
        Some(ProxyAuth::new(t))
    }

    /// The SOCKS5 username/password check.
    pub fn check_user_pass(&self, user: &[u8], pass: &[u8]) -> bool {
        // Both compared, whatever the first says (no early exit on the user name).
        let u = ct_eq(user, USER.as_bytes());
        let p = ct_eq(pass, self.hex.as_bytes());
        self.count(u & p)
    }

    /// The HTTP check: exactly one `Proxy-Authorization` header with the right value.
    pub fn check_headers(&self, headers: &[(String, String)]) -> bool {
        let mut found = None;
        for (k, v) in headers {
            if k.eq_ignore_ascii_case("proxy-authorization") {
                if found.is_some() {
                    return self.count(false);
                }
                found = Some(v.as_str());
            }
        }
        let ok = found.is_some_and(|v| ct_eq(v.as_bytes(), self.basic.as_bytes()));
        self.count(ok)
    }

    fn count(&self, ok: bool) -> bool {
        if ok { &self.ok } else { &self.refused }.fetch_add(1, Ordering::Relaxed);
        ok
    }

    /// `(accepted, refused)` credential checks so far (debug builds show them for test T2).
    pub fn counts(&self) -> (u64, u64) {
        (self.ok.load(Ordering::Relaxed), self.refused.load(Ordering::Relaxed))
    }
}

impl std::fmt::Debug for ProxyAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProxyAuth(<redacted>)")
    }
}

/// Equal length and equal bytes, in time that depends only on the lengths.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u64;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0xff);
        diff |= u64::from(x ^ y);
    }
    std::hint::black_box(diff) == 0
}

/// SOCKS5 reply codes (RFC 1928 §6).
const REP_OK: u8 = 0;
const REP_FAILURE: u8 = 1;
const REP_NOT_ALLOWED: u8 = 2;
const REP_HOST_UNREACHABLE: u8 = 4;
const REP_REFUSED: u8 = 5;
const REP_TTL_EXPIRED: u8 = 6;
const REP_COMMAND: u8 = 7;
const REP_ADDRESS_TYPE: u8 = 8;

impl OpenFail {
    /// The SOCKS5 reply for this failure. Gecko shows 1, 2 and 5 as "connection refused" and
    /// 4 as "unknown host"; Krunk's error page decides from its own VPN state.
    fn socks_reply(self) -> u8 {
        match self {
            OpenFail::NotConnected => REP_FAILURE,
            OpenFail::NotRouted => REP_NOT_ALLOWED,
            OpenFail::NameNotFound => REP_HOST_UNREACHABLE,
            OpenFail::DnsTimeout | OpenFail::TimedOut => REP_TTL_EXPIRED,
            OpenFail::Refused => REP_REFUSED,
        }
    }
}

async fn reply(client: &mut TcpStream, code: u8) -> std::io::Result<()> {
    client.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}

/// Greeting and RFC 1929 credential; true when the client may go on.
async fn authenticate(client: &mut TcpStream, auth: &ProxyAuth) -> std::io::Result<bool> {
    // The version byte was peeked by the caller and is still in the stream.
    let mut hdr = [0u8; 2];
    client.read_exact(&mut hdr).await?;
    let mut methods = vec![0u8; usize::from(hdr[1])];
    client.read_exact(&mut methods).await?;
    if hdr[0] != 5 || !methods.contains(&2) {
        // Only username/password is ever accepted; anything else is closed without a reply.
        return Ok(false);
    }
    client.write_all(&[5, 2]).await?;
    let mut ver_len = [0u8; 2];
    client.read_exact(&mut ver_len).await?;
    let mut user = Zeroizing::new(vec![0u8; usize::from(ver_len[1])]);
    client.read_exact(&mut user).await?;
    let mut plen = [0u8; 1];
    client.read_exact(&mut plen).await?;
    let mut pass = Zeroizing::new(vec![0u8; usize::from(plen[0])]);
    client.read_exact(&mut pass).await?;
    let ok = ver_len[0] == 1 && auth.check_user_pass(&user, &pass);
    if ok {
        client.write_all(&[1, 0]).await?;
    }
    // A wrong credential: closed without the RFC 1929 failure status.
    Ok(ok)
}

/// The CONNECT request after authentication: `(host, port)`, or the reply code to refuse with.
async fn request(client: &mut TcpStream) -> std::io::Result<Result<(Host, u16), u8>> {
    let mut h = [0u8; 4];
    client.read_exact(&mut h).await?;
    if h[0] != 5 {
        return Ok(Err(REP_FAILURE));
    }
    let host = match h[3] {
        1 => {
            let mut a = [0u8; 4];
            client.read_exact(&mut a).await?;
            Some(Host::Ip(IpAddr::V4(Ipv4Addr::from(a))))
        }
        3 => {
            let mut len = [0u8; 1];
            client.read_exact(&mut len).await?;
            let mut name = vec![0u8; usize::from(len[0])];
            client.read_exact(&mut name).await?;
            std::str::from_utf8(&name).ok().and_then(Host::parse)
        }
        4 => {
            let mut a = [0u8; 16];
            client.read_exact(&mut a).await?;
            Some(Host::Ip(IpAddr::V6(Ipv6Addr::from(a))))
        }
        _ => return Ok(Err(REP_ADDRESS_TYPE)),
    };
    let mut port = [0u8; 2];
    client.read_exact(&mut port).await?;
    let port = u16::from_be_bytes(port);
    if h[1] != 1 {
        // BIND and UDP ASSOCIATE are not offered.
        return Ok(Err(REP_COMMAND));
    }
    match host {
        Some(host) if port != 0 => Ok(Ok((host, port))),
        _ => Ok(Err(REP_HOST_UNREACHABLE)),
    }
}

/// One SOCKS5 client (first byte `0x05`, still unread).
pub(crate) async fn serve_client(mut client: TcpStream, shared: Arc<Shared>, auth: Arc<ProxyAuth>) {
    match tokio::time::timeout(AUTH_TIMEOUT, authenticate(&mut client, &auth)).await {
        Ok(Ok(true)) => {}
        _ => return,
    }
    let (host, port) = match tokio::time::timeout(REQUEST_TIMEOUT, request(&mut client)).await {
        Ok(Ok(Ok(t))) => t,
        Ok(Ok(Err(code))) => {
            let _ = reply(&mut client, code).await;
            return;
        }
        _ => return,
    };
    if !shared.is_connected() {
        let _ = reply(&mut client, REP_FAILURE).await;
        return;
    }
    let opened = tokio::select! {
        r = crate::proxy::open(&shared, &host, port) => r,
        _ = shared.disconnected() => Err(OpenFail::NotConnected),
    };
    let upstream = match opened {
        Ok(s) => s,
        Err(fail) => {
            let fail = crate::proxy::settle(&shared, fail).await;
            if fail.is_refusal() {
                shared.note_refused(&host.to_string());
            }
            let _ = reply(&mut client, fail.socks_reply()).await;
            return;
        }
    };
    if reply(&mut client, REP_OK).await.is_err() {
        return;
    }
    // When the tunnel goes down the connection is reset at once (see proxy::handle), so the
    // engine does not wait for its own timeout on a pooled connection.
    let cut = tokio::select! {
        cut = crate::proxy::pipe_tunnel(&mut client, upstream) => cut,
        _ = shared.disconnected() => true,
    };
    if cut {
        crate::proxy::abort(&client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> ProxyAuth {
        let mut t = [0u8; 32];
        for (i, b) in t.iter_mut().enumerate() {
            *b = i as u8 * 7;
        }
        ProxyAuth::new(&t)
    }

    #[test]
    fn hex_and_basic_forms() {
        let a = auth();
        assert_eq!(a.hex.len(), 64);
        assert!(a.hex.starts_with("00070e15"));
        use base64ct::Encoding;
        let want = format!("Basic {}", base64ct::Base64::encode_string(format!("krunk:{}", a.hex.as_str()).as_bytes()));
        assert_eq!(a.basic.as_str(), want);
        assert!(ProxyAuth::from_slice(&[0u8; 31]).is_none());
        assert!(ProxyAuth::from_slice(&[0u8; 33]).is_none());
        assert_eq!(format!("{a:?}"), "ProxyAuth(<redacted>)");
    }

    #[test]
    fn user_pass_missing_wrong_right() {
        let a = auth();
        let hex = a.hex.as_bytes().to_vec();
        assert!(a.check_user_pass(b"krunk", &hex));
        assert!(!a.check_user_pass(b"krunk", b""));
        assert!(!a.check_user_pass(b"", b""));
        assert!(!a.check_user_pass(b"Krunk", &hex));
        let mut wrong = hex.clone();
        wrong[63] ^= 1;
        assert!(!a.check_user_pass(b"krunk", &wrong));
        assert!(!a.check_user_pass(b"krunk", &hex[..63]));
        let mut longer = hex.clone();
        longer.push(b'0');
        assert!(!a.check_user_pass(b"krunk", &longer));
        assert_eq!(a.counts(), (1, 6));
    }

    #[test]
    fn header_missing_wrong_right_duplicate() {
        let a = auth();
        let h = |v: &str| vec![("Proxy-Authorization".to_owned(), v.to_owned())];
        assert!(a.check_headers(&h(a.basic.as_str())));
        assert!(a.check_headers(&[("proxy-authorization".to_owned(), a.basic.to_string())]));
        assert!(!a.check_headers(&[]));
        assert!(!a.check_headers(&h("Basic eA==")));
        assert!(!a.check_headers(&h(&a.basic.to_lowercase())));
        let mut two = h(a.basic.as_str());
        two.extend(h(a.basic.as_str()));
        assert!(!a.check_headers(&two));
    }

    #[test]
    fn constant_time_compare_is_correct() {
        assert!(ct_eq(b"", b""));
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(!ct_eq(b"ab", b"abc"));
        assert!(!ct_eq(b"\xff", b""));
    }

    #[test]
    fn reply_codes() {
        assert_eq!(OpenFail::NotConnected.socks_reply(), 1);
        assert_eq!(OpenFail::NotRouted.socks_reply(), 2);
        assert_eq!(OpenFail::NameNotFound.socks_reply(), 4);
        assert_eq!(OpenFail::Refused.socks_reply(), 5);
        assert_eq!(OpenFail::TimedOut.socks_reply(), 6);
    }
}
