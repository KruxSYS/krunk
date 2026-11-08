//! Configuration from the environment and the secret files (docs/sync/PLAN.md §4.10). Strict: a
//! missing or invalid value stops the server at start-up. Secret values are never printed.
//!
//! Accounts are **fail closed** (OPEN_QUESTIONS Q1/Q2): unless both Turnstile keys and SMTP are
//! configured, every account endpoint answers `503 accounts_disabled`. Cloudflare's Turnstile
//! test secrets and SMTP without TLS (staging: always-pass keys, the Mailpit sink) count only
//! with `ACCOUNTS_TEST_MODE=1`, so a staging configuration that reaches a live deploy by mistake
//! keeps accounts closed instead of opening them without bot protection.

use std::net::SocketAddr;
use std::path::Path;

/// The three server keys from the `secrets` volume (each 32 random bytes).
#[derive(Clone)]
pub struct Keys {
    /// HMAC key for tokens, codes, the CSRF derivation and rate-limit keys.
    pub token_hmac: [u8; 32],
    /// HMAC key applied to passwords before Argon2id (`pepper_id` 1).
    pub password_pepper: [u8; 32],
    /// XChaCha20-Poly1305 key for TOTP secrets at rest.
    pub totp_enc: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmtpTls {
    /// Port 587: plain connection upgraded with STARTTLS (required, not opportunistic).
    StartTls,
    /// Port 465: TLS from the first byte.
    Tls,
    /// No TLS: only for a local test sink (Mailpit on staging), and only without credentials.
    None,
}

#[derive(Clone)]
pub struct Smtp {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub tls: SmtpTls,
    pub from: String,
}

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: SocketAddr,
    pub public_base_url: String,
    /// Public site key of the Turnstile widget (empty: accounts disabled).
    pub turnstile_site_key: String,
    pub turnstile_secret_key: String,
    /// The hostname siteverify must report (default: PUBLIC_BASE_URL's host).
    pub turnstile_hostname: String,
    pub smtp: Option<Smtp>,
    pub keys: Keys,
    pub ip_retention_days: i32,
    /// Most active devices per account (QUOTA_MAX_DEVICES, default 10).
    pub quota_max_devices: i32,
    /// `ACCOUNTS_TEST_MODE=1`: allow the Turnstile test secrets, SMTP without TLS and a staging
    /// PUBLIC_BASE_URL.
    pub test_mode: bool,
    /// PUBLIC_BASE_URL was a staging address outside test mode (replaced by the default).
    pub base_url_rejected: bool,
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
            .field("turnstile_secret_key", &if self.turnstile_secret_key.is_empty() { "" } else { "<redacted>" })
            .field("turnstile_hostname", &self.turnstile_hostname)
            .field("smtp", &self.smtp.as_ref().map(|s| (&s.host, s.port, s.tls)))
            .field("keys", &"<redacted>")
            .field("ip_retention_days", &self.ip_retention_days)
            .field("quota_max_devices", &self.quota_max_devices)
            .field("test_mode", &self.test_mode)
            .field("trusted_peer_host", &self.trusted_peer_host)
            .finish()
    }
}

pub const DEFAULT_BIND: &str = "0.0.0.0:8081";
pub const DEFAULT_SECRETS_DIR: &str = "/run/krunk-secrets";
pub const DEFAULT_PUBLIC_BASE_URL: &str = "https://krunk.209.wf";
const DEFAULT_PUBLIC_HOST: &str = "krunk.209.wf";
pub const DEFAULT_MAIL_FROM: &str = "Krunk <no-reply@krunk.209.wf>";

/// Cloudflare's documented Turnstile test secrets (always pass, always fail, already spent).
pub const TURNSTILE_TEST_SECRETS: [&str; 3] = [
    "1x0000000000000000000000000000000AA",
    "2x0000000000000000000000000000000AA",
    "3x0000000000000000000000000000000AA",
];

pub fn bind_from(get: &dyn Fn(&str) -> Option<String>) -> Result<SocketAddr, String> {
    let bind = get("BIND").unwrap_or_else(|| DEFAULT_BIND.into());
    bind.parse().map_err(|_| format!("BIND is not an address:port: {bind}"))
}

fn is_key_chars(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

fn decode_key(name: &str, value: Option<String>) -> Result<[u8; 32], String> {
    use base64ct::{Base64UrlUnpadded, Encoding};
    let v = value.ok_or_else(|| format!("the secret {name} is missing (the secrets service creates it)"))?;
    let bytes = Base64UrlUnpadded::decode_vec(v.trim()).map_err(|_| format!("the secret {name} is not base64url"))?;
    bytes.try_into().map_err(|_| format!("the secret {name} must be 32 bytes"))
}

impl Config {
    /// True when accounts can work: Turnstile (both keys) and SMTP are configured.
    pub fn accounts_enabled(&self) -> bool {
        self.accounts_blocker().is_none()
    }

    /// Why accounts are closed, for the start-up log; `None` when they are open.
    pub fn accounts_blocker(&self) -> Option<&'static str> {
        if self.base_url_rejected {
            return Some(
                "PUBLIC_BASE_URL is a staging address (http, .test, .localhost, .invalid) without ACCOUNTS_TEST_MODE=1",
            );
        }
        if self.turnstile_site_key.is_empty() || self.turnstile_secret_key.is_empty() {
            return Some("TURNSTILE_SITE_KEY and TURNSTILE_SECRET_KEY are needed");
        }
        let Some(smtp) = &self.smtp else { return Some("SMTP_HOST is needed") };
        if self.turnstile_test_mode() && !self.test_mode {
            return Some("a Cloudflare Turnstile TEST secret is set without ACCOUNTS_TEST_MODE=1");
        }
        if smtp.tls == SmtpTls::None && !self.test_mode {
            return Some("SMTP_TLS=none is set without ACCOUNTS_TEST_MODE=1");
        }
        None
    }

    pub fn turnstile_test_mode(&self) -> bool {
        TURNSTILE_TEST_SECRETS.contains(&self.turnstile_secret_key.as_str())
    }

    /// `scheme://host[:port]` of PUBLIC_BASE_URL, compared with the `Origin` header.
    pub fn public_origin(&self) -> &str {
        let rest = self.public_base_url.split_once("://").map(|(_, r)| r).unwrap_or("");
        let host_len = rest.find('/').unwrap_or(rest.len());
        &self.public_base_url[..self.public_base_url.len() - rest.len() + host_len]
    }

    pub fn from_env() -> Result<Config, String> {
        Config::from_sources(&|k| std::env::var(k).ok(), &crate::secrets::read)
    }

    /// Tests: environment values from `get`, and fixed test keys for the secret files.
    #[cfg(test)]
    pub fn from_lookup(get: &dyn Fn(&str) -> Option<String>) -> Result<Config, String> {
        Config::from_sources(get, &|p| {
            let name = p.file_name()?.to_str()?;
            // 32 bytes of the name's first letter, base64url.
            use base64ct::{Base64UrlUnpadded, Encoding};
            (name != "postgres_password").then(|| Base64UrlUnpadded::encode_string(&[name.as_bytes()[0]; 32]))
        })
    }

    pub fn from_sources(
        get: &dyn Fn(&str) -> Option<String>,
        read_file: &dyn Fn(&Path) -> Option<String>,
    ) -> Result<Config, String> {
        let get = |k: &str| get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let required = |k: &str| get(k).ok_or_else(|| format!("{k} is required"));
        let secrets_dir = get("SECRETS_DIR").unwrap_or_else(|| DEFAULT_SECRETS_DIR.into());
        let secret = |name: &str| read_file(&Path::new(&secrets_dir).join(name));

        // DATABASE_URL (tests, standalone use) or, in the compose stack, the self-provisioned
        // password file plus DB_HOST (default "db").
        let database_url = match get("DATABASE_URL") {
            Some(url) => url,
            None => {
                let file = required("POSTGRES_PASSWORD_FILE")
                    .map_err(|_| "DATABASE_URL or POSTGRES_PASSWORD_FILE is required".to_string())?;
                let password = read_file(Path::new(&file)).ok_or_else(|| format!("cannot read {file}"))?;
                if !password.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                    return Err("the database password must be base64url (init-secrets)".into());
                }
                let host = get("DB_HOST").unwrap_or_else(|| "db".into());
                format!("postgres://krunk:{password}@{host}:5432/krunk")
            }
        };
        if !database_url.starts_with("postgres://") && !database_url.starts_with("postgresql://") {
            return Err("DATABASE_URL must be a postgres:// URL".into());
        }
        let test_mode = match get("ACCOUNTS_TEST_MODE").as_deref() {
            None | Some("0") => false,
            Some("1") => true,
            Some(_) => return Err("ACCOUNTS_TEST_MODE must be 0 or 1".into()),
        };
        let public_base_url =
            get("PUBLIC_BASE_URL").unwrap_or_else(|| DEFAULT_PUBLIC_BASE_URL.into()).trim_end_matches('/').to_string();
        let Some((scheme, rest)) = public_base_url.split_once("://") else {
            return Err("PUBLIC_BASE_URL must start with https:// (or http:// for local tests)".into());
        };
        if scheme != "https" && scheme != "http" {
            return Err("PUBLIC_BASE_URL must start with https:// (or http:// for local tests)".into());
        }
        let authority = rest.split('/').next().unwrap_or("");
        let base_host = authority.rsplit_once(':').map(|(h, _)| h).unwrap_or(authority).to_string();
        if base_host.is_empty() || !is_key_chars(&base_host) {
            return Err("PUBLIC_BASE_URL has no valid host".into());
        }
        // A staging address (plain http, *.test, *.localhost, *.invalid) outside test mode means a
        // staging configuration reached a real deploy: use the real address and keep accounts closed.
        let staging_url = scheme != "https"
            || base_host == "localhost"
            || [".test", ".localhost", ".invalid"].iter().any(|s| base_host.ends_with(s));
        let base_url_rejected = staging_url && !test_mode;
        let (public_base_url, base_host) = if base_url_rejected {
            (DEFAULT_PUBLIC_BASE_URL.to_string(), DEFAULT_PUBLIC_HOST.to_string())
        } else {
            (public_base_url, base_host)
        };

        let turnstile_site_key = get("TURNSTILE_SITE_KEY").unwrap_or_default();
        let turnstile_secret_key = get("TURNSTILE_SECRET_KEY").unwrap_or_default();
        for (name, v) in [("TURNSTILE_SITE_KEY", &turnstile_site_key), ("TURNSTILE_SECRET_KEY", &turnstile_secret_key)]
        {
            if v.len() > 128 || !is_key_chars(v) {
                return Err(format!("{name} has unexpected characters"));
            }
        }
        let turnstile_hostname = get("TURNSTILE_EXPECTED_HOSTNAME").unwrap_or_else(|| base_host.clone());

        let smtp = match get("SMTP_HOST") {
            None => None,
            Some(host) => {
                let tls = match get("SMTP_TLS").as_deref().unwrap_or("starttls") {
                    "starttls" => SmtpTls::StartTls,
                    "tls" => SmtpTls::Tls,
                    "none" => SmtpTls::None,
                    other => return Err(format!("SMTP_TLS must be starttls, tls or none, not {other}")),
                };
                let default_port = match tls {
                    SmtpTls::StartTls => 587,
                    SmtpTls::Tls => 465,
                    SmtpTls::None => 25,
                };
                let port = match get("SMTP_PORT") {
                    Some(p) => p.parse().map_err(|_| "SMTP_PORT is not a port number".to_string())?,
                    None => default_port,
                };
                let user = get("SMTP_USER").unwrap_or_default();
                let password = get("SMTP_PASSWORD").unwrap_or_default();
                if tls == SmtpTls::None && (!user.is_empty() || !password.is_empty()) {
                    return Err("SMTP_TLS=none is for a local test sink only and never sends credentials".into());
                }
                let from = get("MAIL_FROM").unwrap_or_else(|| DEFAULT_MAIL_FROM.into());
                Some(Smtp { host, port, user, password, tls, from })
            }
        };

        let ip_retention_days = match get("IP_RETENTION_DAYS") {
            Some(d) => {
                d.parse::<i32>().ok().filter(|d| (1..=3650).contains(d)).ok_or("IP_RETENTION_DAYS must be 1..3650")?
            }
            None => 90,
        };
        let quota_max_devices = match get("QUOTA_MAX_DEVICES") {
            Some(d) => {
                d.parse::<i32>().ok().filter(|d| (1..=1000).contains(d)).ok_or("QUOTA_MAX_DEVICES must be 1..1000")?
            }
            None => 10,
        };
        let keys = Keys {
            token_hmac: decode_key("token_hmac_key", secret("token_hmac_key"))?,
            password_pepper: decode_key("password_pepper", secret("password_pepper"))?,
            totp_enc: decode_key("totp_enc_key", secret("totp_enc_key"))?,
        };
        let trusted_peer_host = get("TRUSTED_PEER_HOST").unwrap_or_else(|| "web".into());
        Ok(Config {
            database_url,
            bind: bind_from(&get)?,
            public_base_url,
            turnstile_site_key,
            turnstile_secret_key,
            turnstile_hostname,
            smtp,
            keys,
            ip_retention_days,
            quota_max_devices,
            test_mode,
            base_url_rejected,
            trusted_peer_host,
        })
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

    const BASE: [(&str, &str); 2] =
        [("DATABASE_URL", "postgres://u:p@db/k"), ("PUBLIC_BASE_URL", "https://krunk.209.wf")];

    fn with(extra: &[(&'static str, &'static str)]) -> Result<Config, String> {
        let mut v = BASE.to_vec();
        v.extend_from_slice(extra);
        cfg(&v)
    }

    #[test]
    fn required_values() {
        assert!(cfg(&[]).unwrap_err().contains("DATABASE_URL"));
        // From the password file instead.
        let files = |p: &Path| match p.to_str()? {
            "/run/krunk-secrets/postgres_password" => Some("abc_DEF-123".to_string()),
            _ => Some("QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE".to_string()),
        };
        let c = Config::from_sources(
            &|k| match k {
                "POSTGRES_PASSWORD_FILE" => Some("/run/krunk-secrets/postgres_password".into()),
                "PUBLIC_BASE_URL" => Some("https://krunk.209.wf".into()),
                _ => None,
            },
            &files,
        )
        .unwrap();
        assert_eq!(c.database_url, "postgres://krunk:abc_DEF-123@db:5432/krunk");
        assert_eq!(c.keys.token_hmac, [b'A'; 32]);
        let bad = Config::from_sources(
            &|k| match k {
                "POSTGRES_PASSWORD_FILE" => Some("/x".into()),
                "PUBLIC_BASE_URL" => Some("https://x".into()),
                _ => None,
            },
            &|_| Some("p@ss/word".into()),
        );
        assert!(bad.is_err(), "a password that would need URL escaping is refused");
        // Secret files are required.
        let no_keys =
            Config::from_sources(&|k| BASE.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string()), &|_| None);
        assert!(no_keys.unwrap_err().contains("token_hmac_key"));
        assert_eq!(cfg(&[BASE[0]]).unwrap().public_base_url, "https://krunk.209.wf", "the default address");
        let c = cfg(&[BASE[0], ("PUBLIC_BASE_URL", "https://krunk.209.wf/")]).unwrap();
        assert_eq!(c.public_base_url, "https://krunk.209.wf");
        assert_eq!(c.public_origin(), "https://krunk.209.wf");
        assert_eq!(c.turnstile_hostname, "krunk.209.wf");
        assert_eq!(c.bind.to_string(), "0.0.0.0:8081");
        assert_eq!(c.trusted_peer_host, "web");
        assert_eq!(c.ip_retention_days, 90);
        assert_eq!(c.quota_max_devices, 10);
        assert!(!c.accounts_enabled(), "fail closed without Turnstile and SMTP");
        let dbg = format!("{c:?}");
        assert!(!dbg.contains("u:p"), "the database password is never printed");
    }

    #[test]
    fn accounts_need_turnstile_and_smtp() {
        let t = [("TURNSTILE_SITE_KEY", "0x4AAAAAAA"), ("TURNSTILE_SECRET_KEY", "0x4AAAsecret")];
        assert!(!with(&t).unwrap().accounts_enabled());
        assert!(!with(&[("SMTP_HOST", "smtp.example.org")]).unwrap().accounts_enabled());
        let c = with(&[t[0], t[1], ("SMTP_HOST", "smtp.example.org"), ("SMTP_USER", "u"), ("SMTP_PASSWORD", "pw")])
            .unwrap();
        assert!(c.accounts_enabled());
        assert!(!c.turnstile_test_mode());
        let s = c.smtp.as_ref().unwrap();
        assert_eq!((s.port, s.tls, s.from.as_str()), (587, SmtpTls::StartTls, DEFAULT_MAIL_FROM));
        let dbg = format!("{c:?}");
        assert!(!dbg.contains("0x4AAAsecret") && !dbg.contains("pw\""), "{dbg}");
        let test = with(&[("TURNSTILE_SECRET_KEY", "1x0000000000000000000000000000000AA")]).unwrap();
        assert!(test.turnstile_test_mode());
        // Staging values (test keys, plain SMTP to a sink) open accounts only in test mode.
        let staging = [
            ("TURNSTILE_SITE_KEY", "1x00000000000000000000AA"),
            ("TURNSTILE_SECRET_KEY", "1x0000000000000000000000000000000AA"),
            ("SMTP_HOST", "mailpit"),
            ("SMTP_PORT", "1025"),
            ("SMTP_TLS", "none"),
        ];
        let c = with(&staging).unwrap();
        assert!(!c.accounts_enabled());
        assert!(c.accounts_blocker().unwrap().contains("TEST secret"));
        let mut on = staging.to_vec();
        on.push(("ACCOUNTS_TEST_MODE", "1"));
        assert!(with(&on).unwrap().accounts_enabled());
        // Real keys but plain SMTP: closed too.
        let plain = with(&[t[0], t[1], ("SMTP_HOST", "mailpit"), ("SMTP_TLS", "none")]).unwrap();
        assert!(plain.accounts_blocker().unwrap().contains("SMTP_TLS=none"));
        assert!(with(&[("ACCOUNTS_TEST_MODE", "yes")]).is_err());
    }

    #[test]
    fn staging_addresses_need_test_mode() {
        let real = [
            ("TURNSTILE_SITE_KEY", "0x4AAAsite"),
            ("TURNSTILE_SECRET_KEY", "0x4AAAsecret"),
            ("SMTP_HOST", "smtp.example.org"),
        ];
        for url in [
            "https://krunk.test",
            "http://krunk.209.wf",
            "http://dind:10017",
            "https://x.localhost",
            "https://localhost:8080",
            "https://a.invalid",
        ] {
            let mut v = vec![BASE[0], ("PUBLIC_BASE_URL", url)];
            v.extend_from_slice(&real);
            let c = cfg(&v).unwrap();
            assert!(c.base_url_rejected, "{url}");
            assert!(!c.accounts_enabled(), "{url}");
            assert!(c.accounts_blocker().unwrap().contains("PUBLIC_BASE_URL"));
            assert_eq!(c.public_base_url, "https://krunk.209.wf", "{url}: the real address is reported");
            assert_eq!(c.public_origin(), "https://krunk.209.wf");
            assert_eq!(c.turnstile_hostname, "krunk.209.wf");
            v.push(("ACCOUNTS_TEST_MODE", "1"));
            let t = cfg(&v).unwrap();
            assert!(!t.base_url_rejected && t.accounts_enabled(), "{url} in test mode");
            assert_eq!(t.public_base_url, url);
        }
        let mut v = vec![BASE[0], ("PUBLIC_BASE_URL", "https://krunk.example.org")];
        v.extend_from_slice(&real);
        let c = cfg(&v).unwrap();
        assert!(!c.base_url_rejected && c.accounts_enabled());
    }

    #[test]
    fn invalid_values() {
        assert!(cfg(&[("DATABASE_URL", "mysql://x"), BASE[1]]).is_err());
        assert!(cfg(&[BASE[0], ("PUBLIC_BASE_URL", "ftp://x")]).is_err());
        assert!(with(&[("BIND", "nope")]).is_err());
        assert!(with(&[("TURNSTILE_SITE_KEY", "<script>")]).is_err());
        assert!(with(&[("TURNSTILE_SITE_KEY", "1x00000000000000000000AA")]).is_ok());
        assert!(with(&[("SMTP_HOST", "h"), ("SMTP_TLS", "maybe")]).is_err());
        assert!(with(&[("SMTP_HOST", "mailpit"), ("SMTP_TLS", "none"), ("SMTP_USER", "u")]).is_err());
        let sink = with(&[("SMTP_HOST", "mailpit"), ("SMTP_TLS", "none"), ("SMTP_PORT", "1025")]).unwrap();
        assert_eq!(sink.smtp.unwrap().port, 1025);
        assert!(with(&[("IP_RETENTION_DAYS", "0")]).is_err());
        let staging = cfg(&[BASE[0], ("PUBLIC_BASE_URL", "http://dind:10017"), ("ACCOUNTS_TEST_MODE", "1")]).unwrap();
        assert_eq!(staging.public_origin(), "http://dind:10017");
        assert_eq!(staging.turnstile_hostname, "dind");
    }
}
