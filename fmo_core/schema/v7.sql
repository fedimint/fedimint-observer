-- Create the update session_times view
BEGIN;
INSERT INTO schema_version (version)
VALUES (7);

CREATE TABLE nostr_federations (
    event_id bytea PRIMARY KEY,
    federation_id bytea NOT NULL,
    invite_code text NOT NULL,
    event jsonb NOT NULL,
    fetch_time TIMESTAMP NOT NULL
);
