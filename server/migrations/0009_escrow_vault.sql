-- 0.21.1: the recovery root under the password-vault master password (the one secret users type).
ALTER TABLE escrow ADD COLUMN rr_wrap_vault jsonb NULL;
