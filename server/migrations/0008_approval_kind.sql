-- Approval kinds (PROTOCOL §11.6): 'join' is the approval of a new device without keys (§6.2,
-- as before); 'pak' is an already-keyed device that lacks the passwords key asking another
-- device for it, over the same SAS relay. Additive only: one column with a default, so existing
-- rows are 'join' and a rolled-back api (which never names the column) still inserts and reads.
-- (0007 is the expand step in expand/; from 0008 on these are ordinary sqlx migrations, db.rs.)
ALTER TABLE approvals ADD COLUMN kind text NOT NULL DEFAULT 'join' CHECK (kind IN ('join', 'pak'));
