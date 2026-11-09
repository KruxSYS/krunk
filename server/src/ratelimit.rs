//! Rate limits (PROTOCOL §11.2) as fixed windows in Postgres. Unlike an in-memory limiter
//! (`governor`, as PLAN §4.2 first proposed) they survive restarts and also cover the per-day
//! limits. Keys are HMACs of the IP or the normalised email, so the table holds no addresses.
//! The login lockout lives in `login_throttle` (see accounts.rs).

use crate::api::ApiError;
use crate::crypto;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::net::IpAddr;

#[derive(Clone, Copy, Debug)]
pub struct Limit {
    pub bucket: &'static str,
    pub max: i32,
    pub window_s: i64,
}

const fn l(bucket: &'static str, max: i32, window_s: i64) -> Limit {
    Limit { bucket, max, window_s }
}

const MIN: i64 = 60;
const HOUR: i64 = 3600;
const DAY: i64 = 86400;

pub const CONFIG_IP: Limit = l("config/ip", 60, MIN);
pub const REGISTER_IP: Limit = l("register/ip", 5, HOUR);
pub const VERIFY_IP: Limit = l("verify/ip", 20, HOUR);
pub const RESEND_IP: Limit = l("resend/ip", 3, HOUR);
pub const RESEND_EMAIL: Limit = l("resend/email", 3, DAY);
pub const LOGIN_IP: Limit = l("login/ip", 10, 15 * MIN);
pub const RESET_IP: Limit = l("reset/ip", 3, HOUR);
pub const RESET_EMAIL: Limit = l("reset/email", 3, DAY);
pub const RESET_CONFIRM_IP: Limit = l("reset-confirm/ip", 10, HOUR);
pub const PASSWORD_CHANGE: Limit = l("password-change/account", 5, HOUR);
pub const REAUTH: Limit = l("reauth/account", 10, 15 * MIN);
pub const ME: Limit = l("me/session", 60, MIN);
pub const TOTP_SETUP: Limit = l("totp-setup/account", 5, HOUR);
pub const TOTP_ENABLE: Limit = l("totp-enable/account", 10, HOUR);
pub const TOTP_DISABLE: Limit = l("totp-disable/account", 5, HOUR);
// S3 (PROTOCOL §6.1, §11.3, §11.4).
pub const LINK_AUTHORIZE_IP: Limit = l("link-authorize/ip", 10, HOUR);
/// Polls every 5 s for 10 minutes are 120 per grant; this leaves room for a few grants.
pub const LINK_TOKEN_IP: Limit = l("link-token/ip", 600, HOUR);
/// Brute force of user codes: 5 lookups per website session per 10 minutes, 20 per IP per hour.
pub const LINK_LOOKUP_SESSION: Limit = l("link-lookup/session", 5, 10 * MIN);
pub const LINK_LOOKUP_IP: Limit = l("link-lookup/ip", 20, HOUR);
pub const LINK_APPROVE: Limit = l("link-approve/account", 10, HOUR);
pub const REFRESH_DEVICE: Limit = l("refresh/device", 30, HOUR);
pub const REFRESH_IP: Limit = l("refresh/ip", 60, HOUR);
pub const DEVICE_ME: Limit = l("device-me/device", 120, HOUR);
pub const DEVICES_LIST: Limit = l("devices/session", 60, MIN);
pub const DEVICE_REVOKE: Limit = l("device-revoke/account", 20, HOUR);
pub const DEVICE_RENAME: Limit = l("device-rename/account", 30, HOUR);
pub const SESSIONS_LIST: Limit = l("sessions/session", 60, MIN);
pub const SESSION_REVOKE: Limit = l("session-revoke/account", 60, HOUR);
// 0.9.0 (PROTOCOL §11.2).
pub const SESSIONS_REVOKE_ALL: Limit = l("sessions-revoke-all/account", 10, HOUR);
// S6 (PROTOCOL §11.4–§11.6, §11.8).
pub const SYNC_STATE: Limit = l("sync-state/device", 120, HOUR);
pub const KEYS_HEAD: Limit = l("keys-head/device", 120, HOUR);
pub const KEYS_CHAIN: Limit = l("keys-chain/device", 20, HOUR);
pub const KEYS_ESCROW: Limit = l("keys-escrow/account", 5, DAY);
pub const KEYS_COMMIT: Limit = l("keys-commit/device", 60, HOUR);
pub const ROTATION_CLAIM: Limit = l("rotation-claim/device", 20, HOUR);
pub const APPROVALS_CREATE: Limit = l("approvals-create/account", 5, DAY);
pub const APPROVALS_LIST: Limit = l("approvals-list/device", 120, HOUR);
/// One poll per 2 seconds while an approval runs (PROTOCOL §11.6).
pub const APPROVAL_POLL: Limit = l("approval-poll/device", 30, MIN);
pub const APPROVAL_STEP: Limit = l("approval-step/device", 60, HOUR);
pub const DEVICES_LIST_DEVICE: Limit = l("devices/device", 60, MIN);
pub const SESSIONS_LIST_DEVICE: Limit = l("sessions/device", 60, MIN);
// S7 (PROTOCOL §11.7): a full first sync pulls and pushes many pages; a running device every
// 5 minutes uses a few.
pub const RECORDS_READ: Limit = l("records-read/device", 1200, HOUR);
pub const RECORDS_WRITE: Limit = l("records-write/device", 600, HOUR);
pub const REKEY: Limit = l("rekey/device", 600, HOUR);

/// The per-IP key: an IPv4 address, or the /64 network of an IPv6 address. One IPv6
/// customer usually has a whole /64 (or more), so a key per address gave everyone an
/// endless supply of fresh buckets (0.11.7, audit S1). IPv4-mapped addresses count as IPv4.
pub fn ip_key(keys: &crate::config::Keys, ip: IpAddr) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-rl-ip", &[limited_network(ip).as_bytes()])
}

fn limited_network(ip: IpAddr) -> String {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = v6.segments();
                format!("{}/64", std::net::Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
        },
        v4 => v4.to_string(),
    }
}

pub fn email_key(keys: &crate::config::Keys, normalized_email: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-email", &[normalized_email.as_bytes()])
}

pub fn id_key(id: &uuid::Uuid) -> [u8; 32] {
    crypto::sha256(id.as_bytes())
}

fn window_start(now: DateTime<Utc>, window_s: i64) -> (DateTime<Utc>, i64) {
    let t = now.timestamp();
    let start = t - t.rem_euclid(window_s);
    (DateTime::from_timestamp(start, 0).unwrap_or(now), start + window_s - t)
}

/// Counts one request against `limit` for `key`; `rate_limited` (with Retry-After) once the
/// window is full.
pub async fn hit(db: &PgPool, limit: Limit, key: &[u8; 32]) -> Result<(), ApiError> {
    let (start, remaining) = window_start(Utc::now(), limit.window_s);
    let count: i32 = sqlx::query_scalar(
        "INSERT INTO rate_limits (bucket, key, window_start, count) VALUES ($1, $2, $3, 1)
         ON CONFLICT (bucket, key, window_start) DO UPDATE SET count = rate_limits.count + 1
         RETURNING count",
    )
    .bind(limit.bucket)
    .bind(key.as_slice())
    .bind(start)
    .fetch_one(db)
    .await?;
    if count > limit.max {
        tracing::info!(bucket = limit.bucket, "rate limited");
        return Err(ApiError::rate_limited(remaining as u64));
    }
    Ok(())
}

/// `rate_limited` if the window for `key` is already over the limit (a request was refused in
/// it), without counting this request: for requests that count only when they fail.
pub async fn check(db: &PgPool, limit: Limit, key: &[u8; 32]) -> Result<(), ApiError> {
    let (start, remaining) = window_start(Utc::now(), limit.window_s);
    let count: Option<i32> =
        sqlx::query_scalar("SELECT count FROM rate_limits WHERE bucket = $1 AND key = $2 AND window_start = $3")
            .bind(limit.bucket)
            .bind(key.as_slice())
            .bind(start)
            .fetch_optional(db)
            .await?;
    if count.unwrap_or(0) > limit.max {
        tracing::info!(bucket = limit.bucket, "rate limited");
        return Err(ApiError::rate_limited(remaining as u64));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows() {
        let now = DateTime::from_timestamp(1_000_000_123, 0).unwrap();
        let (s, rem) = window_start(now, 60);
        assert_eq!(s.timestamp(), 1_000_000_080);
        assert_eq!(rem, 17);
        let (s, rem) = window_start(now, 86400);
        assert_eq!(s.timestamp() % 86400, 0);
        assert!(rem > 0 && rem <= 86400);
    }

    #[test]
    fn keys_hide_the_values() {
        let keys = crate::config::Keys { token_hmac: [1; 32], password_pepper: [2; 32], totp_enc: [3; 32] };
        let a = ip_key(&keys, "203.0.113.9".parse().unwrap());
        assert_ne!(a, ip_key(&keys, "203.0.113.10".parse().unwrap()));
        assert_ne!(email_key(&keys, "203.0.113.9"), a, "domain-separated");
    }

    /// 0.11.7, audit S1: every address of one IPv6 /64 shares one bucket.
    #[test]
    fn ipv6_counts_per_64() {
        let keys = crate::config::Keys { token_hmac: [1; 32], password_pepper: [2; 32], totp_enc: [3; 32] };
        let k = |s: &str| ip_key(&keys, s.parse().unwrap());
        assert_eq!(k("2001:db8:1:2::1"), k("2001:db8:1:2:ffff:eeee:dddd:cccc"));
        assert_ne!(k("2001:db8:1:2::1"), k("2001:db8:1:3::1"));
        assert_eq!(k("::ffff:203.0.113.9"), k("203.0.113.9"), "mapped IPv4 is IPv4");
        assert_ne!(k("203.0.113.9"), k("203.0.113.10"), "IPv4 stays per address");
    }
}
