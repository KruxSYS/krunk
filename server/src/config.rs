//! Configuration from the environment (docs/sync/PLAN.md §4.10). Strict: a missing or invalid
//! value stops the server at start-up. Secret values are never printed.

use std::net::SocketAddr;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: SocketAddr,
    pub public_base_url: String,
    /// Public site key of the Turnstile widget (may be empty until S2).
    pub turnstile_site_key: String,
    /// Host name of the reverse proxy (Caddy `web`) whose client-IP headers are trusted.
    pub trusted_peer_host: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"<redacted>")
            .field("bind", &self.bind)
            .field("public_base_url", &self.public_base_url)
            .field("turnstile_site_key", &self.turnstile_site_key)
            .field("trusted_peer_host", &self.trusted_peer_host)
            .finish()
    }
}

pub const DEFAULT_BIND: &str = "0.0.0.0:8081";

pub fn bind_from(get: &dyn Fn(&str) -> Option<String>) -> Result<SocketAddr, String> {
    let bind = get("BIND").unwrap_or_else(|| DEFAULT_BIND.into());
    bind.parse().map_err(|_| format!("BIND is not an address:port: {bind}"))
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        Config::from_lookup(&|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: &dyn Fn(&str) -> Option<String>) -> Result<Config, String> {
        let required = |k: &str| get(k).filter(|v| !v.trim().is_empty()).ok_or_else(|| format!("{k} is required"));
        let database_url = required("DATABASE_URL")?;
        if !database_url.starts_with("postgres://") && !database_url.starts_with("postgresql://") {
            return Err("DATABASE_URL must be a postgres:// URL".into());
        }
        let public_base_url = required("PUBLIC_BASE_URL")?.trim_end_matches('/').to_string();
        if !public_base_url.starts_with("https://") && !public_base_url.starts_with("http://") {
            return Err("PUBLIC_BASE_URL must start with https:// (or http:// for local tests)".into());
        }
        let turnstile_site_key = get("TURNSTILE_SITE_KEY").unwrap_or_default();
        if turnstile_site_key.len() > 128
            || !turnstile_site_key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err("TURNSTILE_SITE_KEY has unexpected characters".into());
        }
        let trusted_peer_host = get("TRUSTED_PEER_HOST").unwrap_or_else(|| "web".into());
        Ok(Config { database_url, bind: bind_from(get)?, public_base_url, turnstile_site_key, trusted_peer_host })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> Result<Config, String> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(&move |k| m.get(k).cloned())
    }

    #[test]
    fn required_values() {
        assert!(cfg(&[]).unwrap_err().contains("DATABASE_URL"));
        assert!(cfg(&[("DATABASE_URL", "postgres://u:p@db/k")]).unwrap_err().contains("PUBLIC_BASE_URL"));
        let c = cfg(&[("DATABASE_URL", "postgres://u:p@db/k"), ("PUBLIC_BASE_URL", "https://krunk.209.wf/")]).unwrap();
        assert_eq!(c.public_base_url, "https://krunk.209.wf");
        assert_eq!(c.bind.to_string(), "0.0.0.0:8081");
        assert_eq!(c.trusted_peer_host, "web");
        assert!(!format!("{c:?}").contains("u:p"), "the database password is never printed");
    }

    #[test]
    fn invalid_values() {
        let base = [("DATABASE_URL", "postgres://u:p@db/k"), ("PUBLIC_BASE_URL", "https://x")];
        assert!(cfg(&[("DATABASE_URL", "mysql://x"), base[1]]).is_err());
        assert!(cfg(&[base[0], ("PUBLIC_BASE_URL", "ftp://x")]).is_err());
        assert!(cfg(&[base[0], base[1], ("BIND", "nope")]).is_err());
        assert!(cfg(&[base[0], base[1], ("TURNSTILE_SITE_KEY", "<script>")]).is_err());
        assert!(cfg(&[base[0], base[1], ("TURNSTILE_SITE_KEY", "1x00000000000000000000AA")]).is_ok());
    }
}
