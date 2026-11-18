-- Administration by Nostr key (see fmo_core/src/admin): the keys owners
-- delegated access to, the ids of the signed requests already used, and a
-- record of what admins did.

INSERT INTO schema_version (version)
VALUES (14);

-- Keys with a current grant of delegate access, one row each. Revoking deletes
-- the row; the audit log keeps the history. Owners are configured
-- (FO_ADMIN_NPUBS) and never stored, so restoring a database can't change who
-- owns the observer. The server caps the number of rows.
CREATE TABLE admin_keys (
    -- x-only public key, as in Nostr events
    pubkey BYTEA PRIMARY KEY CHECK (octet_length(pubkey) = 32),
    -- The owner's name for the key holder
    label TEXT NOT NULL CHECK (char_length(label) BETWEEN 1 AND 64),
    -- The owner who granted it; NULL when granted with the FO_ADMIN_AUTH token
    granted_by BYTEA CHECK (octet_length(granted_by) = 32),
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The grant stops working at this time; NULL never expires
    expires_at TIMESTAMPTZ
);

-- Ids of the signed requests (NIP-98 events) admins already sent, kept while
-- the event's timestamp is still acceptable, so a captured Authorization
-- header can't be sent again. Only requests of known admin keys are stored,
-- and expired rows are deleted on every insert, so the table stays small.
CREATE TABLE admin_auth_events (
    event_id BYTEA PRIMARY KEY CHECK (octet_length(event_id) = 32),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX admin_auth_events_expiry ON admin_auth_events (expires_at);

-- What admins did, written before the action runs. Rows are only ever
-- inserted, and their outcome set once.
CREATE TABLE admin_audit_log (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The admin's key; NULL for the FO_ADMIN_AUTH token
    actor BYTEA CHECK (octet_length(actor) = 32),
    actor_role TEXT NOT NULL CHECK (actor_role IN ('owner', 'delegate')),
    action TEXT NOT NULL CHECK (action ~ '^[a-z_]+\.[a-z_]+$'),
    -- What the action was about: a federation id or a public key, in hex
    subject TEXT NOT NULL CHECK (char_length(subject) <= 128),
    -- The action's parameters; never invite codes or other secrets
    detail JSONB NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(detail) = 'object'),
    outcome TEXT NOT NULL DEFAULT 'started' CHECK (outcome IN ('started', 'succeeded', 'failed'))
);

-- A delegate's recent actions of one kind, for its rate limit
CREATE INDEX admin_audit_log_actor ON admin_audit_log (actor, action, at);
