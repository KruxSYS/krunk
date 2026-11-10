//! PostgreSQL pool and migrations (docs/sync/PLAN.md §4.9).
//!
//! Migrations are embedded plain-SQL files from `migrations/`, forward-only. They run at start
//! (`serve`) or on demand (`migrate`). sqlx's migrator takes a Postgres advisory lock for the
//! duration, so only one instance migrates at a time and the others wait.
//!
//! **Expand steps (0.13.0).** The api before 0.13.0 runs sqlx's migrator with `ignore_missing`
//! off: it refuses to start when `_sqlx_migrations` lists a version it doesn't know. A rollback
//! after a new sqlx migration would therefore take the accounts down, however additive the
//! migration. So 0007 is an expand step instead: idempotent, additive DDL from `expand/`, applied
//! after the sqlx migrations under an advisory lock and recorded in `krunk_schema_steps`, which
//! the earlier api never looks at. From 0.13.0 on the migrator ignores unknown applied versions,
//! so later migrations (0008 on) can be ordinary sqlx migrations and still roll back.

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

/// The migrations as the api before 0.13.0 ran them (unknown applied versions refused): the
/// tests use it to show that such an api still starts against this schema.
#[cfg(test)]
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// The expand steps, in order: (name, SQL). Each runs once, in one transaction.
pub const EXPAND: &[(&str, &str)] = &[("0007_account_section", include_str!("../expand/0007_account_section.sql"))];

/// The advisory lock the expand steps hold ("KRUNK").
const EXPAND_LOCK: i64 = 0x4b52554e4b;

pub async fn connect(url: &str) -> Result<PgPool, String> {
    // The database may still be starting: retry for up to a minute.
    let mut last = String::new();
    for attempt in 0..30 {
        match PgPoolOptions::new().max_connections(10).acquire_timeout(Duration::from_secs(5)).connect(url).await {
            Ok(pool) => return Ok(pool),
            Err(e) => {
                last = e.to_string();
                tracing::warn!(attempt, "database not reachable yet");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    // The error text from sqlx contains no password; keep only it.
    Err(format!("cannot connect to the database: {last}"))
}

pub async fn migrate(pool: &PgPool) -> Result<(), String> {
    let mut m = sqlx::migrate!("./migrations");
    // A rolled-back api must start against a schema migrated further (PLAN §4.9).
    m.set_ignore_missing(true);
    m.run(pool).await.map_err(|e| format!("migration failed: {e}"))?;
    expand(pool).await.map_err(|e| format!("expand step failed: {e}"))
}

/// Applies the expand steps not applied yet (see the module comment).
pub async fn expand(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut conn = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)").bind(EXPAND_LOCK).execute(&mut *conn).await?;
    let result = expand_locked(&mut conn).await;
    let unlock = sqlx::query("SELECT pg_advisory_unlock($1)").bind(EXPAND_LOCK).execute(&mut *conn).await;
    result?;
    unlock.map(|_| ())
}

async fn expand_locked(conn: &mut sqlx::PgConnection) -> Result<(), sqlx::Error> {
    use sqlx::Connection;
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS krunk_schema_steps (step text PRIMARY KEY, applied_at timestamptz NOT NULL DEFAULT now())",
    )
    .execute(&mut *conn)
    .await?;
    for (name, sql) in EXPAND {
        let done: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM krunk_schema_steps WHERE step = $1)")
            .bind(name)
            .fetch_one(&mut *conn)
            .await?;
        if done {
            continue;
        }
        let mut tx = conn.begin().await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(*sql)).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO krunk_schema_steps (step) VALUES ($1)").bind(name).execute(&mut *tx).await?;
        tx.commit().await?;
        tracing::info!(step = name, "expand step applied");
    }
    Ok(())
}
