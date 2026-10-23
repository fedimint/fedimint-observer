CREATE TABLE schema_version
(
    version INTEGER
);
INSERT INTO schema_version (version)
VALUES (1);

DROP VIEW session_times;

CREATE MATERIALIZED VIEW session_times AS
WITH session_votes AS (SELECT s.session_index,
                              s.federation_id
                       FROM sessions s),
     sorted_votes AS (SELECT sv.session_index,
                             sv.federation_id,
                             height_vote,
                             ROW_NUMBER()
                             OVER (PARTITION BY sv.federation_id, sv.session_index ORDER BY height_vote) AS rn,
                             COUNT(bhv.height_vote)
                             OVER (PARTITION BY sv.federation_id, sv.session_index)                      AS total_votes
                      FROM session_votes sv
                               LEFT JOIN
                           block_height_votes bhv ON sv.session_index = bhv.session_index
                               AND sv.federation_id = bhv.federation_id),
     session_max_height AS (SELECT session_index,
                                   federation_id,
                                   MAX(height_vote) AS max_height_vote -- Include max to handle NULLs in averaging
                            FROM sorted_votes
                            WHERE total_votes > 0
                            GROUP BY federation_id, session_index),
     session_time AS (SELECT sv.session_index,
                             sv.federation_id,
                             (SELECT bt.timestamp
                              FROM block_times bt
                              WHERE mh.max_height_vote IS NOT NULL
                                AND bt.block_height = mh.max_height_vote) AS timestamp
                      FROM session_votes sv
                               LEFT JOIN
                           session_max_height mh
                           ON sv.session_index = mh.session_index AND sv.federation_id = mh.federation_id),
     grouped_sessions AS (SELECT *,
                                 SUM(CASE WHEN timestamp IS NOT NULL THEN 1 ELSE 0 END)
                                 OVER (PARTITION BY federation_id ORDER BY session_index) AS time_group
                          FROM session_time),
     propagated_times AS (SELECT session_index,
                                 federation_id,
                                 FIRST_VALUE(timestamp)
                                 OVER (PARTITION BY federation_id, time_group ORDER BY session_index) AS estimated_session_timestamp
                          FROM grouped_sessions)
SELECT federation_id,
       session_index,
       estimated_session_timestamp
FROM propagated_times
ORDER BY federation_id, session_index;

create index session_times_federation_id_idx
    on session_times (federation_id);

create unique index session_times_federation_id_session_index_idx
    on session_times (federation_id, session_index);

create index session_times_federation_id_estimated_session_timestamp_idx
    on session_times (federation_id, estimated_session_timestamp);
