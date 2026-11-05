-- S1: the base schema. Later milestones add their own tables (docs/sync/PLAN.md §4.8).
CREATE EXTENSION IF NOT EXISTS citext;

CREATE TABLE app_meta (
    key   text PRIMARY KEY,
    value text NOT NULL
);

INSERT INTO app_meta (key, value) VALUES ('schema_created', now()::text);
