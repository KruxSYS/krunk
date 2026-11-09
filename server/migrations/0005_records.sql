-- S7: the encrypted records (docs/sync/PLAN.md §4.8; PROTOCOL §9, §11.7). Additive only: one
-- new table, so a rolled-back api (0004) still runs against this schema. The server stores
-- envelopes it can't read: the collection name, a keyed record id, the epoch and key kind, the
-- nonce and the ciphertext (padded to buckets, PROTOCOL §9.2). A deletion is a tombstone inside
-- the ciphertext, so it looks like any edit; rows are deleted only when a client purges a
-- tombstone older than 180 days, which raises sync_state.purge_horizon_seq (§9.4, §10.4).
CREATE TABLE records (
    account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    c          text NOT NULL,
    id         bytea NOT NULL CHECK (length(id) = 16),
    -- The account's write order: strictly increasing, allocated from sync_state.head_seq.
    seq        bigint NOT NULL,
    e          integer NOT NULL CHECK (e >= 1),
    k          text NOT NULL CHECK (k IN ('ak', 'pak')),
    envelope   jsonb NOT NULL,
    -- Bytes counted against the quota (the ciphertext).
    size       integer NOT NULL CHECK (size >= 0),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, c, id)
);
CREATE UNIQUE INDEX records_seq ON records (account_id, seq);
CREATE INDEX records_epoch ON records (account_id, k, e);
