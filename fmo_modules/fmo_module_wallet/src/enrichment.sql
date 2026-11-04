CREATE TABLE fmo_wallet.enrichment_jobs (
    id BIGSERIAL PRIMARY KEY,
    on_chain_txid BYTEA NOT NULL UNIQUE
        REFERENCES fmo_wallet.withdrawal_transactions(on_chain_txid) ON DELETE CASCADE,
    attempts INTEGER NOT NULL DEFAULT 0,
    retry_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX enrichment_jobs_due ON fmo_wallet.enrichment_jobs(retry_at, id);
