ALTER TABLE fmo_lnv2.gateways
    ADD COLUMN routing_info JSONB,
    ADD COLUMN routing_observed_at TIMESTAMPTZ,
    ADD COLUMN probed_at TIMESTAMPTZ,
    ADD COLUMN reachable BOOLEAN,
    ADD COLUMN federation_supported BOOLEAN,
    ADD COLUMN latency_ms INTEGER;

CREATE TABLE fmo_lnv2.gateway_keys (
    federation_id BYTEA NOT NULL,
    module_instance_id INTEGER NOT NULL,
    api_endpoint TEXT NOT NULL,
    module_public_key TEXT NOT NULL,
    PRIMARY KEY(federation_id,module_instance_id,api_endpoint,module_public_key),
    FOREIGN KEY(federation_id,module_instance_id,api_endpoint)
        REFERENCES fmo_lnv2.gateways ON DELETE CASCADE
);

CREATE TABLE fmo_lnv2.gateway_health_hourly (
    federation_id BYTEA NOT NULL,
    module_instance_id INTEGER NOT NULL,
    api_endpoint TEXT NOT NULL,
    hour TIMESTAMPTZ NOT NULL,
    samples BIGINT NOT NULL,
    online_samples BIGINT NOT NULL,
    latency_sum_ms BIGINT NOT NULL,
    latency_samples BIGINT NOT NULL,
    PRIMARY KEY (federation_id,module_instance_id,api_endpoint,hour),
    FOREIGN KEY (federation_id,module_instance_id,api_endpoint)
        REFERENCES fmo_lnv2.gateways ON DELETE CASCADE
);

CREATE INDEX gateway_health_retention ON fmo_lnv2.gateway_health_hourly(hour);

CREATE TABLE fmo_lnv2.contract_activity (
    federation_id BYTEA NOT NULL REFERENCES federations(federation_id),
    module_instance_id INTEGER NOT NULL,
    txid BYTEA NOT NULL,
    out_index INTEGER NOT NULL,
    session_index INTEGER NOT NULL,
    gateway_key TEXT NOT NULL,
    direction TEXT NOT NULL CHECK(direction IN ('incoming','outgoing')),
    amount_msat BIGINT NOT NULL CHECK(amount_msat>=0),
    PRIMARY KEY(federation_id,module_instance_id,txid,out_index)
);
CREATE INDEX contract_activity_gateway ON fmo_lnv2.contract_activity
    (federation_id,module_instance_id,gateway_key,session_index);
CREATE INDEX contract_activity_session ON fmo_lnv2.contract_activity
    (federation_id,module_instance_id,session_index);
CREATE TABLE fmo_lnv2.contract_resolutions (
    federation_id BYTEA NOT NULL REFERENCES federations(federation_id),
    module_instance_id INTEGER NOT NULL,
    funding_txid BYTEA NOT NULL,
    funding_out_index INTEGER NOT NULL,
    session_index INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK(outcome IN ('claimed','refunded','cancelled','incoming_spent')),
    PRIMARY KEY(federation_id,module_instance_id,funding_txid,funding_out_index)
);
-- Backfill v2 contract activity from stored sessions; other module cursors stay put.
UPDATE module_cursors SET next_session=0 WHERE module_kind='lnv2';
