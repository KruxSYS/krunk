-- S6: sync keys, escrow and device approvals (docs/sync/PLAN.md §4.8; PROTOCOL §4–§7, §11.4–
-- §11.6). Additive only: new tables, and three new columns on devices with defaults, so a
-- rolled-back api (0.8.0, S3) still runs against this schema. Everything cascades from accounts.
-- The server stores only what it can't read: directory bytes and their MACs (exactly as sent;
-- never re-serialized), the passphrase and recovery-key wraps of the recovery root, and the
-- approval relay fields. It never sees a key.

-- The account directory (ak_dir) and the passwords/escrow directory (pak_dir), one row per
-- version. `hash` = SHA-256(bytes): the CAS of keys/commit and the next version's prev_hash.
-- `epoch` is the AK epoch (ak_dirs) or the PAK epoch (pak_dirs), parsed from the bytes.
CREATE TABLE ak_dirs (
    account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    seq        bigint NOT NULL CHECK (seq >= 1),
    epoch      integer NOT NULL CHECK (epoch >= 1),
    bytes      bytea NOT NULL,
    hash       bytea NOT NULL CHECK (length(hash) = 32),
    mac        bytea NOT NULL CHECK (length(mac) = 32),
    chain      bytea CHECK (length(chain) = 32),
    escrow_mac bytea CHECK (length(escrow_mac) = 32),
    created_by uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, seq)
);

CREATE TABLE pak_dirs (
    account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    seq        bigint NOT NULL CHECK (seq >= 1),
    epoch      integer NOT NULL CHECK (epoch >= 1),
    bytes      bytea NOT NULL,
    hash       bytea NOT NULL CHECK (length(hash) = 32),
    mac        bytea NOT NULL CHECK (length(mac) = 32),
    chain      bytea CHECK (length(chain) = 32),
    escrow_mac bytea CHECK (length(escrow_mac) = 32),
    created_by uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, seq)
);

-- The recovery root wrapped under the passphrase and under the recovery key (PROTOCOL §5.3),
-- with the passphrase KDF parameters. Served only to authenticated devices, and every fetch is
-- mailed to the account.
CREATE TABLE escrow (
    account_id uuid PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    rr_gen     integer NOT NULL CHECK (rr_gen >= 1),
    rr_wrap_pp jsonb NOT NULL,
    rr_wrap_rk jsonb NOT NULL,
    pp_kdf     jsonb NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- One row per account with a keyset. `keyset_gen` changes only when sync is reset (S10).
-- `rotation_*` is set when a device is removed and cleared by the commit of the next epoch;
-- while it is set, writes of that key kind are frozen (423). The leases are advisory (safety
-- comes from the CAS of keys/commit). The record counters are used from S7 on.
CREATE TABLE sync_state (
    account_id        uuid PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    keyset_gen        uuid NOT NULL,
    current_epoch     integer NOT NULL CHECK (current_epoch >= 1),
    pak_epoch         integer NOT NULL CHECK (pak_epoch >= 1),
    rr_gen            integer NOT NULL CHECK (rr_gen >= 1),
    rotation_ak       boolean NOT NULL DEFAULT false,
    rotation_pak      boolean NOT NULL DEFAULT false,
    lease_ak_device   uuid,
    lease_ak_until    timestamptz,
    lease_pak_device  uuid,
    lease_pak_until   timestamptz,
    head_seq          bigint NOT NULL DEFAULT 0,
    purge_horizon_seq bigint NOT NULL DEFAULT 0,
    records           integer NOT NULL DEFAULT 0,
    bytes             bigint NOT NULL DEFAULT 0,
    history_records   integer NOT NULL DEFAULT 0,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);

-- The relay of a device approval (PROTOCOL §6.2): a new device N without keys and a device A
-- with keys exchange ephemeral keys, a commitment, boxes and a confirmation tag through here.
-- One attempt each; the row is deleted when it completes, is cancelled or expires (10 min).
CREATE TABLE approvals (
    id                 uuid PRIMARY KEY,
    account_id         uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    new_device_id      uuid NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    approver_device_id uuid REFERENCES devices(id) ON DELETE CASCADE,
    step               text NOT NULL CHECK (step IN ('requested', 'responded', 'revealed', 'confirmed', 'done')),
    commit_n           bytea NOT NULL CHECK (length(commit_n) = 32),
    epk_a              bytea CHECK (length(epk_a) = 32),
    nonce_a            bytea CHECK (length(nonce_a) = 32),
    epk_n              bytea CHECK (length(epk_n) = 32),
    nonce_n            bytea CHECK (length(nonce_n) = 32),
    box_n              bytea CHECK (length(box_n) <= 4096),
    tag_n              bytea CHECK (length(tag_n) = 32),
    box_a              bytea CHECK (length(box_a) <= 8192),
    created_at         timestamptz NOT NULL DEFAULT now(),
    expires_at         timestamptz NOT NULL
);
CREATE INDEX approvals_account ON approvals (account_id);
CREATE INDEX approvals_expiry ON approvals (expires_at);

-- Devices: whether the current ak_dir lists them (the lists show "linked, no keys yet"),
-- whether any ak_dir ever did (removing such a device starts a key rotation), and when they
-- last synced (S7).
ALTER TABLE devices ADD COLUMN has_keys boolean NOT NULL DEFAULT false;
ALTER TABLE devices ADD COLUMN had_keys boolean NOT NULL DEFAULT false;
ALTER TABLE devices ADD COLUMN last_sync_at timestamptz;
