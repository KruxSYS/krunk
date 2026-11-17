-- One-step registration (service 2026-11-17): every `web/register` makes one row, whatever the
-- address, so the registering page can poll for its session (`web/register/poll`) and the browser
-- that registered can verify without the password (`__Host-krunk_reg`). Additive only.
CREATE TABLE registrations (
    id                    uuid PRIMARY KEY,
    -- HMACs of the poll token and of the cookie; poll_hash is NULL once the session was handed out.
    poll_hash             bytea UNIQUE NULL,
    cookie_hash           bytea UNIQUE NOT NULL,
    -- NULL when no verify mail was sent (the address was already verified): it never completes.
    account_id            uuid NULL REFERENCES accounts (id) ON DELETE CASCADE,
    -- A later registration of the same unverified address voids this one (it owns the password).
    voided                boolean NOT NULL DEFAULT false,
    -- Where the site goes afterwards (a safeNext() path, checked by the server).
    next                  text NULL,
    -- The Krunk link grant to approve on verification (auto-link), as its device-code HMAC.
    link_device_code_hash bytea NULL,
    verified_at           timestamptz NULL,
    link_outcome          text NULL CHECK (link_outcome IN ('linked', 'unavailable')),
    linked_label          text NULL,
    linked_platform       text NULL,
    created_at            timestamptz NOT NULL DEFAULT now(),
    expires_at            timestamptz NOT NULL
);
CREATE INDEX registrations_account ON registrations (account_id) WHERE account_id IS NOT NULL;
CREATE INDEX registrations_expires ON registrations (expires_at);
