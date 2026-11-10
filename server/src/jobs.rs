//! Housekeeping, hourly (docs/sync/PLAN.md §4.3, OPEN_QUESTIONS Q6):
//! - IP retention: session IPs unused for IP_RETENTION_DAYS (90) are nulled, older IP events
//!   deleted;
//! - website sessions dead (revoked or expired) for 30 days are deleted;
//! - unverified accounts older than 7 days are deleted (their links have long expired);
//! - expired Turnstile hashes, rate-limit windows older than 2 days, idle login throttles,
//!   email tokens 7 days past expiry, and outbox rows 7 days after sending or giving up;
//! - S3: link grants 10 minutes past expiry, approved links whose tokens were never collected
//!   (30 minutes), expired access tokens, device IPs unused for IP_RETENTION_DAYS, devices
//!   whose refresh tokens went 90 days unused (signed out), and revoked devices after 30 days.
//! - S6: expired approvals and rotation leases. A device signed out for being idle that ever
//!   held sync keys starts a key rotation, like any other removal (PROTOCOL §7).
//! - 0.11.7 (audit P1): spent refresh tokens after 120 days, the longest refresh-token life
//!   (90 days idle) plus 30 days, so reuse detection still sees every token that could be live.

use sqlx::PgPool;
use std::time::Duration;

pub async fn run_once(db: &PgPool, ip_retention_days: i32) -> Result<(), sqlx::Error> {
    let steps: [(&str, &str); 21] = [
        (
            "session ips",
            "UPDATE web_sessions SET ip = NULL WHERE ip IS NOT NULL AND last_used_at < now() - make_interval(days => $1)",
        ),
        ("ip events", "DELETE FROM ip_events WHERE at < now() - make_interval(days => $1)"),
        (
            "dead sessions",
            "DELETE FROM web_sessions WHERE revoked_at < now() - interval '30 days'
             OR last_used_at < now() - interval '30 days' OR created_at < now() - interval '37 days'",
        ),
        (
            "unverified accounts",
            "DELETE FROM accounts WHERE email_verified_at IS NULL AND created_at < now() - interval '7 days'",
        ),
        ("turnstile", "DELETE FROM turnstile_seen WHERE expires_at < now()"),
        ("rate limits", "DELETE FROM rate_limits WHERE window_start < now() - interval '2 days'"),
        (
            "login throttle",
            "DELETE FROM login_throttle WHERE updated_at < now() - interval '1 day'
             AND (locked_until IS NULL OR locked_until < now())",
        ),
        ("email tokens", "DELETE FROM email_tokens WHERE expires_at < now() - interval '7 days'"),
        (
            "outbox",
            "DELETE FROM email_outbox WHERE coalesce(sent_at, failed_at) < now() - interval '7 days'",
        ),
        ("link grants", "DELETE FROM link_grants WHERE expires_at < now() - interval '10 minutes'"),
        // 0.13.0: pairings, the mail log (90 days, like the IPs) and the deletion list (30 days,
        // longer than the 14 days of dumps that restore-db.sh may bring back).
        ("pairings", "DELETE FROM pair_grants WHERE expires_at < now() - interval '10 minutes'"),
        ("mail log", "DELETE FROM email_log WHERE queued_at < now() - make_interval(days => $1)"),
        ("deletion list", "DELETE FROM account_deletions WHERE deleted_at < now() - interval '30 days'"),
        (
            "uncollected devices",
            "DELETE FROM devices d WHERE d.created_at < now() - interval '30 minutes'
             AND NOT EXISTS (SELECT 1 FROM refresh_families f WHERE f.device_id = d.id)",
        ),
        ("access tokens", "DELETE FROM access_tokens WHERE expires_at < now()"),
        (
            "device ips",
            "UPDATE devices SET last_ip = NULL WHERE last_ip IS NOT NULL AND last_used_at < now() - make_interval(days => $1)",
        ),
        (
            "idle devices",
            "WITH gone AS (
               UPDATE devices SET revoked_at = now(), has_keys = false WHERE revoked_at IS NULL AND id IN (
                 SELECT device_id FROM refresh_families WHERE revoked_at IS NULL AND rotated_at < now() - interval '90 days')
               RETURNING account_id, had_keys),
             rotate AS (
               UPDATE sync_state SET rotation_ak = true, rotation_pak = true, updated_at = now()
               WHERE account_id IN (SELECT account_id FROM gone WHERE had_keys))
             SELECT 1 FROM gone",
        ),
        ("revoked devices", "DELETE FROM devices WHERE revoked_at < now() - interval '30 days'"),
        ("approvals", "DELETE FROM approvals WHERE expires_at < now()"),
        ("spent refresh tokens", "DELETE FROM refresh_used WHERE used_at < now() - interval '120 days'"),
        (
            "rotation leases",
            "UPDATE sync_state SET
               lease_ak_device = CASE WHEN lease_ak_until < now() THEN NULL ELSE lease_ak_device END,
               lease_ak_until = CASE WHEN lease_ak_until < now() THEN NULL ELSE lease_ak_until END,
               lease_pak_device = CASE WHEN lease_pak_until < now() THEN NULL ELSE lease_pak_device END,
               lease_pak_until = CASE WHEN lease_pak_until < now() THEN NULL ELSE lease_pak_until END
             WHERE lease_ak_until < now() OR lease_pak_until < now()",
        ),
    ];
    for (name, sql) in steps {
        let mut q = sqlx::query(sql);
        if sql.contains("$1") {
            q = q.bind(ip_retention_days);
        }
        let n = q.execute(db).await?.rows_affected();
        if n > 0 {
            tracing::info!(job = name, rows = n, "housekeeping");
        }
    }
    Ok(())
}

pub fn spawn(db: PgPool, ip_retention_days: i32) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = run_once(&db, ip_retention_days).await {
                tracing::error!(error = %e, "housekeeping");
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}
