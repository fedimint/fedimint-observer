CREATE TABLE fmo_lnv2.gateway_polls (
    federation_id BYTEA NOT NULL REFERENCES federations(federation_id),
    module_instance_id INTEGER NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL,
    successful_guardians INTEGER NOT NULL,
    total_guardians INTEGER NOT NULL,
    PRIMARY KEY (federation_id, module_instance_id)
);
CREATE TABLE fmo_lnv2.gateways (
    federation_id BYTEA NOT NULL,
    module_instance_id INTEGER NOT NULL,
    api_endpoint TEXT NOT NULL,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
    announcing_guardians JSONB NOT NULL,
    registered BOOLEAN NOT NULL DEFAULT true,
    PRIMARY KEY (federation_id, module_instance_id, api_endpoint),
    FOREIGN KEY (federation_id, module_instance_id)
        REFERENCES fmo_lnv2.gateway_polls ON DELETE CASCADE
);
