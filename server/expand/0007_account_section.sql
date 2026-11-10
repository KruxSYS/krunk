-- 0.13.0: QR pairing, the open account section and S10 (docs/sync/PLAN.md §7; PROTOCOL §6.5,
-- §11.2). An EXPAND step, applied by db::expand() outside sqlx's _sqlx_migrations: the api that
-- ran before 0.13.0 refuses to start when _sqlx_migrations lists a version it doesn't know, so a
-- rollback after a sqlx migration 0007 would take the accounts down. Additive and idempotent:
-- new tables and nullable columns only, nothing dropped or renamed, every statement safe to run
-- again. Existing rows are untouched, and the earlier api keeps working against this schema.

-- "Add a device": the signed-in browser shows a QR code of <base>/pair#<token>; a phone claims it
-- (natively through link/pair/claim, or in a browser through web/pair/claim) and the browser
-- approves after comparing confirm_code. One pairing per account (a new one replaces it), 5
-- minutes, single use. Only the HMACs of the token and of the phone's poll token are stored.
CREATE TABLE IF NOT EXISTS pair_grants (
    id              uuid PRIMARY KEY,
    account_id      uuid NOT NULL UNIQUE REFERENCES accounts(id) ON DELETE CASCADE,
    token_hash      bytea NOT NULL UNIQUE,
    started_session uuid,
    created_at      timestamptz NOT NULL DEFAULT now(),
    expires_at      timestamptz NOT NULL,
    status          text NOT NULL DEFAULT 'waiting'
                    CHECK (status IN ('waiting', 'claimed', 'approved', 'denied')),
    claim_kind      text CHECK (claim_kind IN ('device', 'web')),
    claimed_at      timestamptz,
    confirm_code    text CHECK (confirm_code ~ '^[0-9]{6}$'),
    platform        text CHECK (length(platform) <= 16),
    label           text CHECK (length(label) <= 64),
    client_version  text CHECK (length(client_version) <= 32),
    ip              inet,
    country         text CHECK (country IS NULL OR length(country) = 2),
    user_agent      text CHECK (length(user_agent) <= 256),
    poll_hash       bytea UNIQUE,
    last_poll_at    timestamptz
);
CREATE INDEX IF NOT EXISTS pair_grants_expiry ON pair_grants (expires_at);

-- A native claim becomes an ordinary pending link grant (polled through link/token as before),
-- tied to its pairing. Its user_code_hash is random and never shown, so /link can't reach it.
ALTER TABLE link_grants ADD COLUMN IF NOT EXISTS pair_id uuid REFERENCES pair_grants(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS link_grants_pair ON link_grants (pair_id);

-- How a website session was signed in: NULL (sessions from before 0.13.0) and 'password' are the
-- sign-in form, 'qr' a phone approved by QR.
ALTER TABLE web_sessions ADD COLUMN IF NOT EXISTS via text CHECK (via IN ('password', 'qr'));

-- Where an event came from, for the Activity list: a website session or a device (no foreign
-- key, so an event outlives the session or device it names, which then shows as removed).
ALTER TABLE ip_events ADD COLUMN IF NOT EXISTS source_kind text CHECK (source_kind IN ('web', 'device'));
ALTER TABLE ip_events ADD COLUMN IF NOT EXISTS source_id uuid;

-- Which mails the account was sent and when (for the data download): the template name only,
-- never the address, parameters, links or tokens. Kept 90 days.
CREATE TABLE IF NOT EXISTS email_log (
    id         bigserial PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    template   text NOT NULL,
    queued_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS email_log_account ON email_log (account_id, queued_at);
CREATE INDEX IF NOT EXISTS email_log_at ON email_log (queued_at);

-- Deleted account ids (PLAN §4.9, §7): the backup service exports them to deletions.log beside
-- the dumps, and restore-db.sh deletes them again after a restore. No foreign key (the account is
-- gone) and no address. Kept 30 days, longer than the 14 days of dumps.
CREATE TABLE IF NOT EXISTS account_deletions (
    account_id uuid PRIMARY KEY,
    deleted_at timestamptz NOT NULL DEFAULT now()
);
