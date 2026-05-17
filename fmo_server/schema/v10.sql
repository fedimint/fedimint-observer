BEGIN;

INSERT INTO
    schema_version (version)
VALUES
    (10);

ALTER TABLE guardian_health
    ADD COLUMN IF NOT EXISTS software_version TEXT;

COMMIT;
