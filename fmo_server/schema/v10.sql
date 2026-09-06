BEGIN;

INSERT INTO schema_version (version)
VALUES (10);

CREATE TABLE guardian_api_announcements
(
    federation_id BYTEA   NOT NULL REFERENCES federations (federation_id),
    guardian_id   INTEGER NOT NULL CHECK (guardian_id >= 0 AND guardian_id <= 65535),
    -- Fixed-width big-endian u64 bytes retain numeric ordering under BYTEA
    -- comparison without truncating values above PostgreSQL BIGINT's range.
    nonce         BYTEA   NOT NULL CHECK (octet_length(nonce) = 8),
    announcement  BYTEA   NOT NULL,
    PRIMARY KEY (federation_id, guardian_id)
);

COMMIT;
