-- S3: device linking (RFC 8628 pattern) and device tokens (docs/sync/PLAN.md §4.3, §4.8;
-- PROTOCOL §6.1, §11.3, §11.4). Additive only: new tables, nothing in S1/S2 changes, so a
-- rolled-back api still runs against this schema. Everything cascades from accounts (and from
-- devices), so deleting an account stays one statement.

-- A linked device. The id is minted when a link grant is approved on the website; `short` is
-- the next free u16 of the account (HLC tie-breaks, PROTOCOL §3.1). `label` is the client's
-- non-secret name ("Krunk 0.8.0 on Windows 11"), which the website can rename until the
-- encrypted device_meta name arrives with sync (S6).
CREATE TABLE devices (
    id             uuid PRIMARY KEY,
    account_id     uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    short          integer NOT NULL CHECK (short BETWEEN 0 AND 65535),
    platform       text NOT NULL CHECK (platform IN ('windows', 'android')),
    label          text NOT NULL CHECK (length(label) BETWEEN 1 AND 64),
    client_version text NOT NULL CHECK (length(client_version) <= 32),
    created_at     timestamptz NOT NULL DEFAULT now(),
    last_used_at   timestamptz NOT NULL DEFAULT now(),
    last_ip        inet,
    last_country   text,
    user_agent     text CHECK (length(user_agent) <= 256),
    revoked_at     timestamptz,
    UNIQUE (account_id, short)
);
CREATE INDEX devices_account ON devices (account_id);

-- Refresh tokens rotate on every use; one family per device. Only HMACs are stored: the
-- current one here, the spent ones in refresh_used (presenting one again revokes the family).
CREATE TABLE refresh_families (
    id           uuid PRIMARY KEY,
    device_id    uuid NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    current_hash bytea NOT NULL UNIQUE,
    created_at   timestamptz NOT NULL DEFAULT now(),
    rotated_at   timestamptz NOT NULL DEFAULT now(),
    revoked_at   timestamptz
);
CREATE INDEX refresh_families_device ON refresh_families (device_id);

CREATE TABLE refresh_used (
    hash      bytea PRIMARY KEY,
    family_id uuid NOT NULL REFERENCES refresh_families(id) ON DELETE CASCADE,
    used_at   timestamptz NOT NULL DEFAULT now()
);

-- Access tokens: 15 minutes, HMAC only.
CREATE TABLE access_tokens (
    hash       bytea PRIMARY KEY,
    device_id  uuid NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL
);
CREATE INDEX access_tokens_device ON access_tokens (device_id);

-- Link grants: 10 minutes, single use. HMACs of both codes; the requester's IP and country
-- for the approval page. Deleted once answered, and by expires_at + 10 minutes at the latest.
CREATE TABLE link_grants (
    device_code_hash bytea PRIMARY KEY,
    user_code_hash   bytea NOT NULL UNIQUE,
    platform         text NOT NULL CHECK (platform IN ('windows', 'android')),
    label            text NOT NULL CHECK (length(label) BETWEEN 1 AND 64),
    client_version   text NOT NULL CHECK (length(client_version) <= 32),
    ip               inet,
    country          text,
    user_agent       text CHECK (length(user_agent) <= 256),
    created_at       timestamptz NOT NULL DEFAULT now(),
    expires_at       timestamptz NOT NULL,
    interval_s       integer NOT NULL DEFAULT 5,
    last_poll_at     timestamptz,
    status           text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'denied')),
    account_id       uuid REFERENCES accounts(id) ON DELETE CASCADE,
    device_id        uuid REFERENCES devices(id) ON DELETE CASCADE
);
CREATE INDEX link_grants_expiry ON link_grants (expires_at);
