-- Delivered after commit. Startup also drains work created while offline.
CREATE FUNCTION notify_session_time_repair() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('fmo_work', 'session-time-repair');
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER notify_session_time_repair
    AFTER INSERT OR UPDATE ON session_time_repairs
    FOR EACH STATEMENT EXECUTE FUNCTION notify_session_time_repair();
CREATE TRIGGER notify_repair_block_arrival
    AFTER INSERT ON block_times
    FOR EACH STATEMENT EXECUTE FUNCTION notify_session_time_repair();

-- BEGIN SESSION TIME REPAIR BATCHING
-- Workers re-apply this section at startup (it is idempotent), so databases
-- that ran v14 before it was added get it too.
--
-- Coalesce session time repairs. Each ready job used to cost a full
-- rebuild_read_models, so a federation whose votes outran the block explorer
-- by N heights rebuilt its whole history N times, holding the federation's
-- read-model lock against live ingestion each time. Now one call claims every
-- ready job of one federation and rebuilds once, from the earliest session.
--
-- Unchanged guarantees:
-- * Jobs are deleted in the rebuild's transaction, so an error or crash
--   leaves every claimed job queued for the next attempt.
-- * Rows another transaction holds (a vote upserting its job) are skipped,
--   never waited on, and stay queued. A vote whose upsert meets a claimed row
--   waits for this transaction and then inserts a fresh job after it commits.
CREATE OR REPLACE FUNCTION process_session_time_repair() RETURNS BOOLEAN AS $$
DECLARE
    fed BYTEA;
    start_index INTEGER;
BEGIN
    SELECT r.federation_id INTO fed FROM session_time_repairs r
    WHERE r.block_height = -1 OR EXISTS
        (SELECT 1 FROM block_times b WHERE b.block_height = r.block_height)
    ORDER BY r.block_height, r.federation_id
    LIMIT 1 FOR UPDATE OF r SKIP LOCKED;
    IF NOT FOUND THEN RETURN FALSE; END IF;
    -- The row locked above is ours, so SKIP LOCKED keeps it in this claim.
    WITH claimed AS (
        SELECT r.block_height FROM session_time_repairs r
        WHERE r.federation_id = fed AND (r.block_height = -1 OR EXISTS
            (SELECT 1 FROM block_times b WHERE b.block_height = r.block_height))
        FOR UPDATE OF r SKIP LOCKED
    ), done AS (
        DELETE FROM session_time_repairs r USING claimed c
        WHERE r.federation_id = fed AND r.block_height = c.block_height
        RETURNING r.from_index
    )
    SELECT min(from_index) INTO start_index FROM done;
    PERFORM rebuild_read_models(fed, start_index);
    RETURN TRUE;
END;
$$ LANGUAGE plpgsql;
-- END SESSION TIME REPAIR BATCHING

INSERT INTO schema_version(version) VALUES (14);
