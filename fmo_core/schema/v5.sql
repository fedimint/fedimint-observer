-- Create the update session_times view
BEGIN;
INSERT INTO schema_version (version)
VALUES (5);

DROP MATERIALIZED VIEW IF EXISTS session_times;

CREATE MATERIALIZED VIEW session_times AS
WITH proposer_votes AS (
    SELECT
        federation_id,
        session_index,
        proposer,
        MAX(height_vote) AS proposer_height
    FROM block_height_votes
    GROUP BY federation_id, session_index, proposer
),

session_proposer_heights AS (
    SELECT
        federation_id,
        session_index,
        proposer_height,
        COUNT(*) AS vote_cnt
    FROM proposer_votes
    GROUP BY federation_id, session_index, proposer_height
),

session_heights AS (
    SELECT
        federation_id,
        session_index,
        proposer_height AS block_height,
        vote_cnt,
        ROW_NUMBER()
            OVER (
                PARTITION BY federation_id, session_index ORDER BY vote_cnt DESC
            )
        AS rn
    FROM session_proposer_heights
),

session_times AS (
    SELECT
        sh.federation_id,
        sh.session_index,
        sh.block_height,
        bt.timestamp,
        sh.vote_cnt
    FROM session_heights AS sh
    LEFT JOIN
        block_times AS bt
        ON sh.block_height = bt.block_height
    WHERE sh.rn = 1
)

SELECT
    s.federation_id,
    s.session_index,
    MAX(st.timestamp)
        OVER (
            PARTITION BY s.federation_id
            ORDER BY
                s.session_index
            ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
        )
    AS estimated_session_timestamp
FROM sessions AS s
LEFT JOIN
    session_times AS st
    ON s.federation_id = st.federation_id AND s.session_index = st.session_index
ORDER BY s.federation_id, s.session_index;

CREATE INDEX session_times_federation_id_idx ON session_times (federation_id);

CREATE UNIQUE INDEX session_times_federation_id_session_index_idx ON session_times (
    federation_id, session_index
);

CREATE INDEX session_times_federation_id_estimated_session_timestamp_idx ON session_times (
    federation_id, estimated_session_timestamp
);

COMMIT;
