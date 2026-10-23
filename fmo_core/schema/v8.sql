-- raw Consensus Items and Transaction details tables
BEGIN;

INSERT INTO
    schema_version (version)
VALUES
    (8);

CREATE TABLE IF NOT EXISTS consensus_items (
    federation_id BYTEA NOT NULL REFERENCES federations (federation_id),
    session_index INTEGER NOT NULL,
    item_index INTEGER NOT NULL,
    proposer INTEGER NOT NULL,
    kind TEXT NOT NULL,
    data JSONB NOT NULL,
    FOREIGN KEY (federation_id, session_index) REFERENCES sessions (federation_id, session_index),
    PRIMARY KEY (federation_id, session_index, item_index)
);

CREATE INDEX IF NOT EXISTS federation_consensus_items ON consensus_items (federation_id);

CREATE INDEX IF NOT EXISTS federation_session_consensus_items ON consensus_items (federation_id, session_index);

CREATE TABLE IF NOT EXISTS transaction_input_details (
    federation_id BYTEA NOT NULL REFERENCES federations (federation_id),
    txid BYTEA NOT NULL,
    in_index INTEGER NOT NULL,
    kind TEXT NOT NULL,
    details JSONB NOT NULL,
    PRIMARY KEY (federation_id, txid, in_index),
    FOREIGN KEY (federation_id, txid) REFERENCES transactions (federation_id, txid) -- This might be a bit too heavy of a foreign key? Maybe use rowid instead?
);

CREATE INDEX IF NOT EXISTS federation_input_details ON transaction_input_details (federation_id);

CREATE INDEX IF NOT EXISTS federation_transaction_input_details ON transaction_input_details (federation_id, txid);

CREATE INDEX IF NOT EXISTS federation_kind_input_details ON transaction_input_details (federation_id, kind);

CREATE TABLE IF NOT EXISTS transaction_output_details (
    federation_id BYTEA NOT NULL REFERENCES federations (federation_id),
    txid BYTEA NOT NULL,
    out_index INTEGER NOT NULL,
    kind TEXT NOT NULL,
    details JSONB NOT NULL,
    PRIMARY KEY (federation_id, txid, out_index),
    FOREIGN KEY (federation_id, txid) REFERENCES transactions (federation_id, txid) -- This might be a bit too heavy of a foreign key? Maybe use rowid instead?
);

CREATE INDEX IF NOT EXISTS federation_output_details ON transaction_output_details (federation_id);

CREATE INDEX IF NOT EXISTS federation_transaction_output_details ON transaction_output_details (federation_id, txid);

CREATE INDEX IF NOT EXISTS federation_kind_output_details ON transaction_output_details (federation_id, kind);

COMMIT;
