CREATE TABLE fmo_walletv2.wallets (
    federation_id BYTEA NOT NULL REFERENCES federations(federation_id),
    module_instance_id INTEGER NOT NULL,
    balance_msat BIGINT,
    balance_observed_at TIMESTAMPTZ,
    history_observed_at TIMESTAMPTZ,
    history_end_index BIGINT NOT NULL DEFAULT -1,
    history_revision BIGINT NOT NULL DEFAULT 0,
    history_collected_revision BIGINT NOT NULL DEFAULT -1,
    PRIMARY KEY (federation_id, module_instance_id)
);
CREATE TABLE fmo_walletv2.chain (
    federation_id BYTEA NOT NULL,
    module_instance_id INTEGER NOT NULL,
    tx_index BIGINT NOT NULL CHECK (tx_index >= 0),
    data JSONB NOT NULL,
    PRIMARY KEY (federation_id, module_instance_id, tx_index),
    FOREIGN KEY (federation_id, module_instance_id)
        REFERENCES fmo_walletv2.wallets ON DELETE CASCADE
);
CREATE TABLE fmo_walletv2.deposit_jobs (
    federation_id BYTEA NOT NULL,
    module_instance_id INTEGER NOT NULL,
    txid BYTEA NOT NULL,
    in_index INTEGER NOT NULL,
    session_index INTEGER NOT NULL,
    output_index BIGINT NOT NULL CHECK (output_index >= 0),
    fee_sat BIGINT NOT NULL CHECK (fee_sat >= 0),
    attempts INTEGER NOT NULL DEFAULT 0,
    retry_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (federation_id, txid, in_index),
    FOREIGN KEY (federation_id, txid, in_index)
        REFERENCES transaction_inputs ON DELETE CASCADE
);
CREATE INDEX deposit_jobs_due ON fmo_walletv2.deposit_jobs
    (federation_id, module_instance_id, retry_at, output_index);
CREATE INDEX deposit_jobs_output_index ON fmo_walletv2.deposit_jobs
    (federation_id, module_instance_id, output_index);
-- Replay only this module to discover deposit indices in existing stored sessions.
-- Core ingest and other module cursors continue independently.
UPDATE module_cursors SET next_session = 0 WHERE module_kind = 'walletv2';
