INSERT INTO schema_version (version)
VALUES (3);

CREATE TABLE IF NOT EXISTS guardian_health
(
    federation_id BYTEA     NOT NULL REFERENCES federations (federation_id),
    time     TIMESTAMP NOT NULL,
    guardian_id   INTEGER   NOT NULL,
    status        JSONB,
    block_height  INTEGER,
    latency_ms    INTEGER,
    PRIMARY KEY (federation_id, guardian_id, time)
);

CREATE INDEX IF NOT EXISTS guardian_health_federation_time ON guardian_health (federation_id, time);

CREATE OR REPLACE VIEW latest_guardian_health AS
WITH latest_federation_times AS (
    SELECT
        federation_id,
        MAX(time) as latest_time
    FROM
        guardian_health
    GROUP BY
        federation_id
)
SELECT
    gh.federation_id,
    gh.time,
    gh.guardian_id,
    gh.status,
    gh.block_height,
    gh.latency_ms
FROM
    guardian_health gh
        INNER JOIN
    latest_federation_times lft
    ON
        gh.federation_id = lft.federation_id
            AND gh.time = lft.latest_time;
