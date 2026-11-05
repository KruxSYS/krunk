//! PostgreSQL pool and migrations (docs/sync/PLAN.md §4.9).
//!
//! Migrations are embedded plain-SQL files from `migrations/`, forward-only. They run at start
//! (`serve`) or on demand (`migrate`). sqlx's migrator takes a Postgres advisory lock for the
//! duration, so only one instance migrates at a time and the others wait.

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

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
    MIGRATOR.run(pool).await.map_err(|e| format!("migration failed: {e}"))
}
