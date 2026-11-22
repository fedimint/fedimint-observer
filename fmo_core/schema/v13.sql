-- Processes page guardian route rows by revision; other kinds keep NULL.
ALTER TABLE federation_observations
    ADD COLUMN revision BIGINT;

-- Locked until commit, so revision order is visibility order (unlike a
-- sequence). Also holds the worker epoch that fences handoff.
CREATE TABLE guardian_endpoint_write_state (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    worker_epoch BIGINT NOT NULL DEFAULT 0,
    last_revision BIGINT NOT NULL
);

-- Pre-release builds stored {url, nonce, announcement}; keep the announcement.
UPDATE federation_observations o
SET data = (
    SELECT jsonb_object_agg(
        peer,
        CASE WHEN jsonb_typeof(route) = 'object' AND route ? 'announcement'
             THEN route -> 'announcement'
             ELSE route
        END
    )
    FROM jsonb_each(o.data) AS routes(peer, route)
)
WHERE o.kind = 'guardian_endpoints'
  AND jsonb_typeof(o.data) = 'object'
  AND EXISTS (
      SELECT 1
      FROM jsonb_each(o.data) AS routes(peer, route)
      WHERE jsonb_typeof(route) = 'object' AND route ? 'announcement'
  );

-- Route rows written by a pre-release build without revisions.
UPDATE federation_observations o
SET revision = numbered.revision
FROM (
    SELECT federation_id,
           row_number() OVER (ORDER BY observed_at, federation_id) AS revision
    FROM federation_observations
    WHERE kind = 'guardian_endpoints'
) numbered
WHERE o.federation_id = numbered.federation_id
  AND o.kind = 'guardian_endpoints';

INSERT INTO guardian_endpoint_write_state(last_revision)
SELECT COALESCE(MAX(revision), 0)
FROM federation_observations
WHERE kind = 'guardian_endpoints';

CREATE INDEX federation_observations_kind_revision
    ON federation_observations(kind, revision);

-- Rejects route writes that don't carry the current worker epoch.
CREATE FUNCTION enforce_guardian_endpoint_worker_epoch() RETURNS trigger AS $$
DECLARE
    requested_epoch BIGINT;
    active_epoch BIGINT;
BEGIN
    requested_epoch := NULLIF(
        current_setting('fmo.guardian_endpoint_worker_epoch', TRUE),
        ''
    )::BIGINT;
    SELECT worker_epoch INTO active_epoch
    FROM guardian_endpoint_write_state
    WHERE singleton = TRUE;

    IF requested_epoch IS NULL OR requested_epoch IS DISTINCT FROM active_epoch THEN
        RAISE EXCEPTION 'guardian endpoint write has stale or missing worker epoch';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER federation_observations_guardian_endpoint_worker_epoch
    BEFORE INSERT OR UPDATE ON federation_observations
    FOR EACH ROW
    WHEN (NEW.kind = 'guardian_endpoints')
    EXECUTE FUNCTION enforce_guardian_endpoint_worker_epoch();

INSERT INTO schema_version(version) VALUES (13);
