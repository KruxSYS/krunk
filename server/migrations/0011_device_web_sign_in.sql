-- Website sign-in from a signed-in Krunk (service 2026-11-17): Krunk trades its device token for
-- a single-use code (60 s) when it opens the website's sign-in page; the page trades the code
-- for a website session (via 'device'). Only the code's HMAC is stored.
CREATE TABLE web_sign_in_codes (
    hash       bytea PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    device_id  uuid NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL
);
CREATE INDEX web_sign_in_codes_expires ON web_sign_in_codes (expires_at);
-- A session can now also be signed in "by Krunk" (the list says which device). Widening the
-- check only: every existing row ('password', 'qr' or NULL) still passes, and an older api
-- writes only those.
-- (`via` comes from the expand step 0007, which runs after these migrations: on a fresh database
-- it doesn't exist yet, so it is made here; the expand step's ADD COLUMN IF NOT EXISTS then skips.)
ALTER TABLE web_sessions ADD COLUMN IF NOT EXISTS via text;
ALTER TABLE web_sessions DROP CONSTRAINT IF EXISTS web_sessions_via_check;
ALTER TABLE web_sessions ADD CONSTRAINT web_sessions_via_check CHECK (via IN ('password', 'qr', 'device'));
ALTER TABLE web_sessions ADD COLUMN via_device uuid NULL REFERENCES devices (id) ON DELETE SET NULL;
