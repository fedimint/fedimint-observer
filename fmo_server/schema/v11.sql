-- LNv2 gateway discovery: track which Lightning module protocol a gateway is registered under
BEGIN;

INSERT INTO schema_version (version)
VALUES (11);

-- Existing rows all came from the LNv1 (`ln`) module registry. LNv2 rows use the normalized
-- gateway API URL as `gateway_id`, since the LNv2 registry only lists URLs.
ALTER TABLE gateways
    ADD COLUMN IF NOT EXISTS protocol TEXT NOT NULL DEFAULT 'lnv1';

ALTER TABLE gateways DROP CONSTRAINT gateways_pkey;
ALTER TABLE gateways ADD PRIMARY KEY (federation_id, protocol, gateway_id);

-- LNv2 identity comes from probing the gateway, which may fail
ALTER TABLE gateways ALTER COLUMN node_pub_key DROP NOT NULL;
ALTER TABLE gateways ALTER COLUMN lightning_alias DROP NOT NULL;

-- LNv2 only: result of the most recent `routing_info` probe
--   'ok'          - the gateway returned routing info for this federation
--   'not_serving' - the gateway answered but does not serve this federation
--   'unreachable' - the probe failed or timed out
ALTER TABLE gateways
    ADD COLUMN IF NOT EXISTS module_public_key TEXT,
    ADD COLUMN IF NOT EXISTS routing_status TEXT,
    ADD COLUMN IF NOT EXISTS routing_checked_at TIMESTAMPTZ;

ALTER TABLE gateway_poll_snapshots
    ADD COLUMN IF NOT EXISTS protocol TEXT NOT NULL DEFAULT 'lnv1';

ALTER TABLE gateway_poll_snapshots DROP CONSTRAINT gateway_poll_snapshots_pkey;
ALTER TABLE gateway_poll_snapshots ADD PRIMARY KEY (federation_id, protocol, gateway_id, poll_time);

-- `is_seen` remains registry presence; `routing_ok` is LNv2 routing readiness (NULL for LNv1)
ALTER TABLE gateway_poll_snapshots
    ADD COLUMN IF NOT EXISTS routing_ok BOOLEAN;

-- Every LNv2 module key ever observed for a gateway, so that contracts can still be
-- attributed after a gateway rotates its key or leaves the registry.
CREATE TABLE IF NOT EXISTS lnv2_gateway_keys (
    federation_id        BYTEA       NOT NULL REFERENCES federations (federation_id),
    gateway_id           TEXT        NOT NULL,
    module_public_key    TEXT        NOT NULL,
    lightning_public_key TEXT        NOT NULL,
    first_seen           TIMESTAMPTZ NOT NULL,
    last_seen            TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (federation_id, gateway_id, module_public_key)
);

CREATE INDEX IF NOT EXISTS lnv2_gateway_keys_module_public_key
    ON lnv2_gateway_keys (federation_id, module_public_key);

COMMIT;
