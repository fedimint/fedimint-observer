-- Optimize latest_guardian_health to avoid aggregating all historical health rows.
BEGIN;

INSERT INTO schema_version (version)
VALUES (9);

CREATE OR REPLACE VIEW latest_guardian_health AS
SELECT
    gh.federation_id,
    gh.time,
    gh.guardian_id,
    gh.status,
    gh.block_height,
    gh.latency_ms
FROM
    federations f
        CROSS JOIN LATERAL (
            SELECT
                time
            FROM
                guardian_health
            WHERE
                federation_id = f.federation_id
            ORDER BY
                time DESC
            LIMIT 1
        ) latest
        INNER JOIN
    guardian_health gh
    ON
        gh.federation_id = f.federation_id
            AND gh.time = latest.time;

COMMIT;
