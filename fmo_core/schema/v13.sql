-- Observations from data sources other than the federation's guardians:
-- compiled-in collectors and external observability services that push over
-- HTTP (see fmo_core/src/source). One row per federation, source and kind,
-- holding the newest observation only, so the table is bounded by the
-- observed federations times the kinds the build declares.

INSERT INTO schema_version (version)
VALUES (13);

CREATE TABLE source_observations (
    federation_id BYTEA NOT NULL REFERENCES federations(federation_id) ON DELETE CASCADE,
    -- Mirrors the validation of the source registry, so a bug there can't
    -- store names the API and the retention task don't expect
    source TEXT NOT NULL CHECK (source ~ '^[a-z][a-z0-9_]{0,31}$'),
    kind TEXT NOT NULL CHECK (kind ~ '^[a-z][a-z0-9_.-]{0,63}$'),
    data JSONB NOT NULL CHECK (jsonb_typeof(data) = 'object'),
    -- When the source observed it, as the source says
    observed_at TIMESTAMPTZ NOT NULL,
    -- When the observer stored it
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (federation_id, source, kind)
);

-- Retention deletes by source, kind and age
CREATE INDEX source_observations_retention ON source_observations (source, kind, observed_at);
