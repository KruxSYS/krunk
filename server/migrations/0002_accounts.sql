-- S2: accounts, email tokens, the mail outbox, web sessions, TOTP backup codes, Turnstile
-- replay cache, rate-limit windows and IP events (docs/sync/PLAN.md §4.2, §4.3, §4.8).
-- Every account-owned row cascades from accounts, so deleting an account is one statement.

CREATE TABLE accounts (
    id                uuid PRIMARY KEY,
    email             citext NOT NULL UNIQUE CHECK (length(email) <= 254),
    email_verified_at timestamptz,
    pw_phc            text NOT NULL,
    pepper_id         smallint NOT NULL,
    pw_changed_at     timestamptz NOT NULL DEFAULT now(),
    -- XChaCha20-Poly1305 under totp_enc_key: nonce (24) || ciphertext; AAD = the account id.
    totp_secret_enc   bytea,
    totp_pending_enc  bytea,
    totp_enabled      boolean NOT NULL DEFAULT false,
    -- The last accepted TOTP time step; codes at or before it are refused (no replay).
    totp_last_step    bigint NOT NULL DEFAULT 0,
    created_at        timestamptz NOT NULL DEFAULT now(),
    last_active_at    timestamptz NOT NULL DEFAULT now()
);

-- Single-use backup codes for TOTP: HMAC-SHA256(token_hmac_key, code), 60-bit random codes.
CREATE TABLE backup_codes (
    account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    code_hash  bytea NOT NULL,
    used_at    timestamptz,
    PRIMARY KEY (account_id, code_hash)
);

-- Verification and reset links: only HMAC-SHA256(token_hmac_key, token) is stored.
CREATE TABLE email_tokens (
    hash       bytea PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       text NOT NULL CHECK (kind IN ('verify', 'reset')),
    expires_at timestamptz NOT NULL,
    used_at    timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX email_tokens_account ON email_tokens (account_id, kind);

-- Mail is queued in the same transaction as the change it reports, then sent by a worker.
-- params (which may hold a link with a token) are cleared once the mail is sent; sent rows
-- are deleted after 7 days.
CREATE TABLE email_outbox (
    id              bigserial PRIMARY KEY,
    account_id      uuid REFERENCES accounts(id) ON DELETE SET NULL,
    to_addr         text NOT NULL,
    template        text NOT NULL,
    params          jsonb NOT NULL DEFAULT '{}',
    attempts        int NOT NULL DEFAULT 0,
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    sent_at         timestamptz,
    failed_at       timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX email_outbox_due ON email_outbox (next_attempt_at) WHERE sent_at IS NULL AND failed_at IS NULL;

-- Website sessions: the cookie holds 32 random bytes; the row holds their HMAC. The CSRF token
-- is derived from the cookie token, so it isn't stored.
CREATE TABLE web_sessions (
    id           uuid PRIMARY KEY,
    account_id   uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    token_hash   bytea NOT NULL UNIQUE,
    created_at   timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz NOT NULL DEFAULT now(),
    reauth_at    timestamptz,
    ip           inet,
    country      text CHECK (country IS NULL OR length(country) = 2),
    user_agent   text CHECK (user_agent IS NULL OR length(user_agent) <= 256),
    revoked_at   timestamptz
);
CREATE INDEX web_sessions_account ON web_sessions (account_id);

-- Turnstile tokens already presented (SHA-256), kept 10 minutes.
CREATE TABLE turnstile_seen (
    hash       bytea PRIMARY KEY,
    expires_at timestamptz NOT NULL
);

-- Fixed-window rate limits. key is an HMAC of the IP or the normalised email, never the value
-- itself, so this table holds no addresses. Rows are deleted two days after their window.
CREATE TABLE rate_limits (
    bucket       text NOT NULL,
    key          bytea NOT NULL,
    window_start timestamptz NOT NULL,
    count        int NOT NULL,
    PRIMARY KEY (bucket, key, window_start)
);

-- Failed logins per normalised email (HMAC), whether or not an account exists, so a lockout
-- reveals nothing: 5 failures, then 1, 2, 4 ... 60 minutes.
CREATE TABLE login_throttle (
    email_hash   bytea PRIMARY KEY,
    failures     int NOT NULL DEFAULT 0,
    locked_until timestamptz,
    updated_at   timestamptz NOT NULL DEFAULT now()
);

-- Security-relevant events with their IP; IPs are purged after IP_RETENTION_DAYS (90).
CREATE TABLE ip_events (
    id         bigserial PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       text NOT NULL,
    ip         inet,
    country    text,
    at         timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ip_events_account ON ip_events (account_id, at);
