//! Email: an outbox in Postgres and a worker that sends it over SMTP (docs/sync/PLAN.md §4.3,
//! OPEN_QUESTIONS Q2/Q3/Q11).
//!
//! A mail is queued in the same transaction as the change it reports, so it is sent exactly
//! when that change commits. The worker leases due rows (`FOR UPDATE SKIP LOCKED`), sends them,
//! and on failure retries with back-off (1, 2, 4 … 120 minutes; 8 attempts). Once a mail is sent
//! or given up, its `params` (which may hold a link with a token) are cleared. The templates are
//! short English plain text in `server/templates/`, with no tracking and no images. Logs carry
//! the outbox id and template, never the address or the content.

use crate::config::{Smtp, SmtpTls};
use lettre::message::header::ContentType;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    Verify,
    RegisterExists,
    Reset,
    PasswordChanged,
    TotpEnabled,
    TotpDisabled,
    DeviceLinked,
    DeviceRevoked,
    TokenReuse,
}

impl Template {
    pub fn name(self) -> &'static str {
        match self {
            Template::Verify => "verify",
            Template::RegisterExists => "register_exists",
            Template::Reset => "reset",
            Template::PasswordChanged => "password_changed",
            Template::TotpEnabled => "totp_enabled",
            Template::TotpDisabled => "totp_disabled",
            Template::DeviceLinked => "device_linked",
            Template::DeviceRevoked => "device_revoked",
            Template::TokenReuse => "token_reuse",
        }
    }

    pub fn from_name(n: &str) -> Option<Template> {
        Template::ALL.into_iter().find(|t| t.name() == n)
    }

    pub const ALL: [Template; 9] = [
        Template::Verify,
        Template::RegisterExists,
        Template::Reset,
        Template::PasswordChanged,
        Template::TotpEnabled,
        Template::TotpDisabled,
        Template::DeviceLinked,
        Template::DeviceRevoked,
        Template::TokenReuse,
    ];

    fn source(self) -> &'static str {
        match self {
            Template::Verify => include_str!("../templates/verify.txt"),
            Template::RegisterExists => include_str!("../templates/register_exists.txt"),
            Template::Reset => include_str!("../templates/reset.txt"),
            Template::PasswordChanged => include_str!("../templates/password_changed.txt"),
            Template::TotpEnabled => include_str!("../templates/totp_enabled.txt"),
            Template::TotpDisabled => include_str!("../templates/totp_disabled.txt"),
            Template::DeviceLinked => include_str!("../templates/device_linked.txt"),
            Template::DeviceRevoked => include_str!("../templates/device_revoked.txt"),
            Template::TokenReuse => include_str!("../templates/token_reuse.txt"),
        }
    }
}

/// `(subject, body)`. Placeholders: `{{site}}` and any string in `params` (e.g. `{{link}}`).
pub fn render(t: Template, params: &Value, site: &str) -> (String, String) {
    let src = t.source();
    let (first, rest) = src.split_once('\n').unwrap_or((src, ""));
    let subject = first.strip_prefix("Subject: ").unwrap_or(first).trim().to_string();
    let mut body = rest.trim_start_matches('\n').to_string();
    body.push_str("\n-- \nKrunk · {{site}}\nPrivacy: {{site}}/privacy\n");
    if let Some(map) = params.as_object() {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                body = body.replace(&format!("{{{{{k}}}}}"), s);
            }
        }
    }
    (subject, body.replace("{{site}}", site))
}

/// Queues a mail inside the caller's transaction.
pub async fn enqueue(
    conn: &mut PgConnection,
    account_id: Option<uuid::Uuid>,
    to: &str,
    t: Template,
    params: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO email_outbox (account_id, to_addr, template, params) VALUES ($1, $2, $3, $4)")
        .bind(account_id)
        .bind(to)
        .bind(t.name())
        .bind(params)
        .execute(conn)
        .await?;
    Ok(())
}

pub const MAX_ATTEMPTS: i32 = 8;

/// Minutes to wait after the given (1-based) failed attempt.
pub fn backoff_minutes(attempt: i32) -> i64 {
    (1i64 << (attempt.clamp(1, 8) - 1)).min(120)
}

#[derive(Clone)]
pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl Mailer {
    pub fn new(cfg: &Smtp) -> Result<Mailer, String> {
        crate::crypto::ensure_tls_provider();
        let mut b = match cfg.tls {
            SmtpTls::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host),
            SmtpTls::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host),
            SmtpTls::None => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host)),
        }
        .map_err(|e| format!("SMTP: {e}"))?
        .port(cfg.port)
        .timeout(Some(Duration::from_secs(20)));
        if !cfg.user.is_empty() {
            b = b.credentials(Credentials::new(cfg.user.clone(), cfg.password.clone()));
        }
        let from: Mailbox = cfg.from.parse().map_err(|e| format!("MAIL_FROM: {e}"))?;
        Ok(Mailer { transport: b.build(), from })
    }

    async fn send(&self, to: &str, subject: &str, body: String) -> Result<(), String> {
        let to: Mailbox = to.parse().map_err(|_| "invalid recipient".to_string())?;
        let msg = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body)
            .map_err(|e| format!("message: {e}"))?;
        self.transport.send(msg).await.map(|_| ()).map_err(|e| format!("smtp: {e}"))
    }
}

#[derive(sqlx::FromRow)]
struct Due {
    id: i64,
    to_addr: String,
    template: String,
    params: Value,
    attempts: i32,
}

/// One pass over the due mails; returns how many were sent.
pub async fn send_due(db: &PgPool, mailer: &Mailer, site: &str) -> Result<usize, sqlx::Error> {
    // Lease: a crashed worker's rows come back after 10 minutes.
    let due: Vec<Due> = sqlx::query_as(
        "UPDATE email_outbox SET attempts = attempts + 1, next_attempt_at = now() + interval '10 minutes'
         WHERE id IN (SELECT id FROM email_outbox
                      WHERE sent_at IS NULL AND failed_at IS NULL AND next_attempt_at <= now()
                      ORDER BY id LIMIT 10 FOR UPDATE SKIP LOCKED)
         RETURNING id, to_addr, template, params, attempts",
    )
    .fetch_all(db)
    .await?;
    let mut sent = 0;
    for m in due {
        let Some(t) = Template::from_name(&m.template) else {
            tracing::error!(outbox_id = m.id, template = %m.template, "mail: unknown template");
            sqlx::query("UPDATE email_outbox SET failed_at = now(), params = '{}' WHERE id = $1")
                .bind(m.id)
                .execute(db)
                .await?;
            continue;
        };
        let (subject, body) = render(t, &m.params, site);
        match mailer.send(&m.to_addr, &subject, body).await {
            Ok(()) => {
                sent += 1;
                tracing::info!(outbox_id = m.id, template = t.name(), "mail sent");
                sqlx::query("UPDATE email_outbox SET sent_at = now(), params = '{}' WHERE id = $1")
                    .bind(m.id)
                    .execute(db)
                    .await?;
            }
            Err(e) if m.attempts >= MAX_ATTEMPTS => {
                tracing::error!(outbox_id = m.id, template = t.name(), error = %e, "mail: giving up");
                sqlx::query("UPDATE email_outbox SET failed_at = now(), params = '{}' WHERE id = $1")
                    .bind(m.id)
                    .execute(db)
                    .await?;
            }
            Err(e) => {
                let wait = backoff_minutes(m.attempts);
                tracing::warn!(outbox_id = m.id, template = t.name(), attempt = m.attempts, retry_in_min = wait, error = %e, "mail: send failed");
                sqlx::query(
                    "UPDATE email_outbox SET next_attempt_at = now() + make_interval(mins => $2) WHERE id = $1",
                )
                .bind(m.id)
                .bind(wait as i32)
                .execute(db)
                .await?;
            }
        }
    }
    Ok(sent)
}

pub fn spawn_worker(db: PgPool, mailer: Mailer, site: String) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = send_due(&db, &mailer, &site).await {
                tracing::error!(error = %e, "mail worker");
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_render() {
        for t in Template::ALL {
            let (subject, body) = render(
                t,
                &serde_json::json!({"link": "https://x/account/verify?token=T", "label": "Krunk 0.8.0 on Windows 11",
                                    "platform": "Windows", "country": "DE", "time": "2026-11-08 12:00 UTC"}),
                "https://krunk.209.wf",
            );
            assert!(!subject.is_empty() && !subject.contains('\n') && subject.len() < 80, "{t:?}");
            assert!(!body.contains("{{"), "{t:?} leaves a placeholder: {body}");
            assert!(body.contains("https://krunk.209.wf/privacy"));
            assert!(!body.contains("<") && !body.contains("http://"), "plain text, https links only");
            assert_eq!(Template::from_name(t.name()), Some(t));
        }
        let (_, body) =
            render(Template::Verify, &serde_json::json!({"link": "https://k/account/verify?token=abc"}), "https://k");
        assert!(body.contains("https://k/account/verify?token=abc"));
    }

    #[test]
    fn backoff() {
        assert_eq!((1..=9).map(backoff_minutes).collect::<Vec<_>>(), vec![1, 2, 4, 8, 16, 32, 64, 120, 120]);
    }
}
