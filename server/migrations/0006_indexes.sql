-- 0.11.7 (audit P1): indexes only, so this is additive and safe on existing data. A rolled-back
-- api (0005) runs against this schema unchanged. Existing accounts, sessions and devices see no
-- change. The tables are small, so building the indexes inside the migration's transaction
-- (no CONCURRENTLY) holds their locks only briefly.
--
-- Foreign keys whose cascades (deleting a device or an account) scanned the whole child table:
CREATE INDEX IF NOT EXISTS refresh_used_family ON refresh_used (family_id);
CREATE INDEX IF NOT EXISTS email_outbox_account ON email_outbox (account_id);
CREATE INDEX IF NOT EXISTS approvals_new_device ON approvals (new_device_id);
CREATE INDEX IF NOT EXISTS approvals_approver_device ON approvals (approver_device_id);
CREATE INDEX IF NOT EXISTS link_grants_account ON link_grants (account_id);
CREATE INDEX IF NOT EXISTS link_grants_device ON link_grants (device_id);
-- The hourly housekeeping (jobs.rs) deletes a small slice of a table kept for months:
CREATE INDEX IF NOT EXISTS ip_events_at ON ip_events (at);
-- (Not indexed, on measurement: rate_limits, access_tokens and turnstile_seen lose most of their
-- rows at every run and are written on hot paths, so an index only adds write cost.
-- refresh_families has one row per device. See evidence-0.11.7/perf.)
-- refresh_used holds the spent refresh tokens for reuse detection (devices.rs `refresh`): a
-- spent token presented again revokes its device. jobs.rs now deletes rows older than 120 days.
-- That is the longest refresh-token life (90 days idle, REFRESH_IDLE_DAYS) plus 30 days, so
-- every token that could still be live keeps its row.
CREATE INDEX IF NOT EXISTS refresh_used_at ON refresh_used (used_at);
