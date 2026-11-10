//! Session logs (0.15.0, `krunk://sessions`): per browser run, the sites visited, the time each
//! tab and site was in the foreground, and Krunk's CPU/GPU use over time. Off by default
//! (`settings.sessionLog.enabled`); private tabs are never logged. WP-E2 owns this module.

/// Storage V10.
pub(crate) const SCHEMA_V10: &str = "
CREATE TABLE IF NOT EXISTS log_sessions (
    id INTEGER PRIMARY KEY,
    started INTEGER NOT NULL,
    ended INTEGER
);
";
