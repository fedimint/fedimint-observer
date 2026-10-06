BEGIN;

INSERT INTO
    schema_version (version)
VALUES
    (11);

-- Lightning probe measurements pushed by external probers (see fmo_prober).
-- Keyed by LN node rather than federation gateway since one LN node can serve
-- multiple federations, join with `gateways` via `node_pub_key`.
CREATE TABLE IF NOT EXISTS gateway_probes (
    id                   BIGSERIAL   PRIMARY KEY,
    prober_id            TEXT        NOT NULL,
    node_pub_key         TEXT        NOT NULL,
    probe_time           TIMESTAMPTZ NOT NULL,
    amount_msat          BIGINT      NOT NULL CHECK (amount_msat > 0),
    outcome              TEXT        NOT NULL CHECK (outcome IN (
        'success',
        'insufficient_liquidity',
        'unreachable',
        'route_failure',
        'no_route',
        'local_failure',
        'timeout',
        'error'
    )),
    latency_ms           INTEGER,
    failure_code         TEXT,
    failure_source_index INTEGER,
    route_hops           INTEGER,
    route_fee_msat       BIGINT,
    UNIQUE (prober_id, node_pub_key, probe_time, amount_msat)
);

CREATE INDEX IF NOT EXISTS gateway_probes_node_time
    ON gateway_probes (node_pub_key, probe_time DESC);

CREATE INDEX IF NOT EXISTS gateway_probes_time
    ON gateway_probes (probe_time);

COMMIT;
