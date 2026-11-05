//! The client's IP address and country (docs/sync/PLAN.md §4.4).
//!
//! Caddy (`web`) decides the client IP. It trusts `CF-Connecting-IP` only from Cloudflare's
//! ranges and the configured hop, and passes the result as `X-Krunk-Client-IP`. This service
//! trusts that header, and `CF-IPCountry`, **only** when the TCP peer is Caddy itself. Otherwise
//! it uses the peer address and no country.

use axum::http::HeaderMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::{Arc, RwLock};
use std::time::Duration;

pub const CLIENT_IP_HEADER: &str = "x-krunk-client-ip";
pub const COUNTRY_HEADER: &str = "cf-ipcountry";

/// Addresses of the trusted reverse proxy, refreshed from DNS.
#[derive(Clone, Default)]
pub struct TrustedPeers(Arc<RwLock<Vec<IpAddr>>>);

impl TrustedPeers {
    pub fn set(&self, ips: Vec<IpAddr>) {
        if let Ok(mut g) = self.0.write() {
            *g = ips;
        }
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        self.0.read().map(|g| g.iter().any(|t| same_ip(*t, ip))).unwrap_or(false)
    }

    /// Resolves `host` now and then every 60 s (container IPs change on redeploys).
    pub fn spawn_refresh(&self, host: String) {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                match tokio::net::lookup_host((host.as_str(), 0)).await {
                    Ok(addrs) => me.set(addrs.map(|a| a.ip()).collect()),
                    Err(e) => tracing::warn!(host = %host, error = %e, "cannot resolve the trusted proxy"),
                }
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });
    }
}

fn same_ip(a: IpAddr, b: IpAddr) -> bool {
    canonical(a) == canonical(b)
}

/// IPv4-mapped IPv6 addresses compare equal to their IPv4 form.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    }
}

/// What a request is attributed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInfo {
    pub ip: IpAddr,
    /// Two-letter country from Cloudflare, if the request came through the trusted proxy.
    pub country: Option<String>,
}

pub fn client_info(peer: IpAddr, headers: &HeaderMap, trusted: &TrustedPeers) -> ClientInfo {
    let peer = canonical(peer);
    if !trusted.contains(peer) {
        return ClientInfo { ip: peer, country: None };
    }
    let ip = headers
        .get(CLIENT_IP_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<IpAddr>().ok())
        .map(canonical)
        .unwrap_or(peer);
    let country = headers
        .get(COUNTRY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_uppercase()))
        .map(String::from);
    ClientInfo { ip, country }
}

/// The address with its host part removed, for logs and listings: /24 for IPv4, /48 for IPv6.
pub fn masked(ip: IpAddr) -> String {
    match canonical(ip) {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            format!("{}.{}.{}.0/24", o[0], o[1], o[2])
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            format!("{}/48", Ipv6Addr::new(s[0], s[1], s[2], 0, 0, 0, 0, 0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(), v.parse().unwrap());
        }
        h
    }

    #[test]
    fn headers_only_from_the_trusted_proxy() {
        let trusted = TrustedPeers::default();
        trusted.set(vec!["172.28.0.2".parse().unwrap()]);
        let h = headers(&[("x-krunk-client-ip", "203.0.113.9"), ("cf-ipcountry", "DE")]);
        // From Caddy: the forwarded values are used.
        assert_eq!(
            client_info("172.28.0.2".parse().unwrap(), &h, &trusted),
            ClientInfo { ip: "203.0.113.9".parse().unwrap(), country: Some("DE".into()) }
        );
        // From anyone else (e.g. a container reaching the API directly): ignored.
        assert_eq!(
            client_info("172.28.0.9".parse().unwrap(), &h, &trusted),
            ClientInfo { ip: "172.28.0.9".parse().unwrap(), country: None }
        );
        // IPv4-mapped peer addresses match too.
        let mapped: IpAddr = "::ffff:172.28.0.2".parse().unwrap();
        assert_eq!(client_info(mapped, &h, &trusted).ip, "203.0.113.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn bad_forwarded_values_fall_back() {
        let trusted = TrustedPeers::default();
        trusted.set(vec!["10.0.0.1".parse().unwrap()]);
        let peer: IpAddr = "10.0.0.1".parse().unwrap();
        let h = headers(&[("x-krunk-client-ip", "1.2.3.4, 5.6.7.8"), ("cf-ipcountry", "de<")]);
        assert_eq!(client_info(peer, &h, &trusted), ClientInfo { ip: peer, country: None });
        assert_eq!(client_info(peer, &HeaderMap::new(), &trusted), ClientInfo { ip: peer, country: None });
    }

    #[test]
    fn masking() {
        assert_eq!(masked("203.0.113.9".parse().unwrap()), "203.0.113.0/24");
        assert_eq!(masked("2001:db8:abcd:12::1".parse().unwrap()), "2001:db8:abcd::/48");
        assert_eq!(masked("::ffff:10.1.2.3".parse().unwrap()), "10.1.2.0/24");
    }
}
