//! Security-relevant events with their IP and country (`ip_events`, PLAN §4.8), shown on the
//! account page's Activity list and in the data download. IPs are purged after 90 days.

use crate::api::Client;
use sqlx::PgConnection;
use uuid::Uuid;

/// Where an event came from (0.13.0): a website session, a linked device, or neither (a link
/// in a mail, a refresh token presented again).
#[derive(Clone, Copy, Debug)]
pub enum Source {
    None,
    Web(Uuid),
    Device(Uuid),
}

impl Source {
    fn parts(self) -> (Option<&'static str>, Option<Uuid>) {
        match self {
            Source::None => (None, None),
            Source::Web(id) => (Some("web"), Some(id)),
            Source::Device(id) => (Some("device"), Some(id)),
        }
    }
}

/// Records one event inside the caller's transaction.
pub async fn record(
    conn: &mut PgConnection,
    account_id: Uuid,
    kind: &str,
    client: &Client,
    source: Source,
) -> Result<(), sqlx::Error> {
    let (source_kind, source_id) = source.parts();
    sqlx::query(
        "INSERT INTO ip_events (account_id, kind, ip, country, source_kind, source_id)
         VALUES ($1, $2, $3::inet, $4, $5, $6)",
    )
    .bind(account_id)
    .bind(kind)
    .bind(client.info.public_ip().map(|ip| ip.to_string()))
    .bind(client.info.country.as_deref())
    .bind(source_kind)
    .bind(source_id)
    .execute(conn)
    .await?;
    Ok(())
}
