-- Request handlers read observations; background workers own remote I/O.
CREATE TABLE federation_observations (
    federation_id BYTEA NOT NULL REFERENCES federations(federation_id),
    kind TEXT NOT NULL,
    data JSONB NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (federation_id, kind)
);
CREATE TABLE federation_session_counts (
    federation_id BYTEA PRIMARY KEY REFERENCES federations(federation_id),
    session_count BIGINT NOT NULL
);
INSERT INTO federation_session_counts
SELECT federation_id, COUNT(*) FROM sessions GROUP BY federation_id;
CREATE FUNCTION count_stored_session() RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO federation_session_counts VALUES (NEW.federation_id, 1)
    ON CONFLICT (federation_id) DO UPDATE
        SET session_count = federation_session_counts.session_count + 1;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER sessions_count_insert AFTER INSERT ON sessions
FOR EACH ROW EXECUTE FUNCTION count_stored_session();
CREATE INDEX federation_daily_activity_day ON federation_daily_activity(day);
CREATE INDEX guardian_health_retention_time ON guardian_health(time);
INSERT INTO schema_version(version) VALUES (13);

-- BEGIN SESSION TIME REPAIR
-- Votes must remain ingestible while the block explorer is behind. Keep a
-- durable repair request until the corresponding block time is available.
DO $$
DECLARE constraint_name TEXT;
BEGIN
    FOR constraint_name IN
        SELECT conname FROM pg_constraint
        WHERE conrelid = 'block_height_votes'::regclass
          AND confrelid = 'block_times'::regclass AND contype = 'f'
    LOOP
        EXECUTE format('ALTER TABLE block_height_votes DROP CONSTRAINT %I', constraint_name);
    END LOOP;
END;
$$;
CREATE TABLE session_time_repairs (
    federation_id BYTEA NOT NULL REFERENCES federations (federation_id),
    block_height INTEGER NOT NULL,
    from_index INTEGER NOT NULL,
    PRIMARY KEY (federation_id, block_height)
);
CREATE INDEX session_time_repairs_height ON session_time_repairs (block_height);

CREATE FUNCTION schedule_missing_block_time_repair() RETURNS trigger AS $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM block_times WHERE block_height = NEW.height_vote) THEN
        INSERT INTO session_time_repairs (federation_id, block_height, from_index)
        VALUES (NEW.federation_id, NEW.height_vote, NEW.session_index)
        ON CONFLICT (federation_id, block_height) DO UPDATE
            SET from_index = LEAST(session_time_repairs.from_index, EXCLUDED.from_index);
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER schedule_missing_block_time_repair
    AFTER INSERT ON block_height_votes
    FOR EACH ROW EXECUTE FUNCTION schedule_missing_block_time_repair();

-- Repair existing read models once, including any previously stale data.
-- -1 is a sentinel for a repair that does not wait for a block timestamp.
INSERT INTO session_time_repairs (federation_id, block_height, from_index)
SELECT federation_id, -1, 0 FROM federations;

-- Row locking and repair share one transaction: rollback preserves the job,
-- and a concurrent vote can safely schedule another repair after it commits.
CREATE FUNCTION process_session_time_repair() RETURNS BOOLEAN AS $$
DECLARE job session_time_repairs%ROWTYPE;
BEGIN
    SELECT r.* INTO job FROM session_time_repairs r
    WHERE r.block_height = -1 OR EXISTS
        (SELECT 1 FROM block_times b WHERE b.block_height = r.block_height)
    ORDER BY r.block_height, r.federation_id
    LIMIT 1 FOR UPDATE OF r SKIP LOCKED;
    IF NOT FOUND THEN RETURN FALSE; END IF;
    PERFORM rebuild_read_models(job.federation_id, job.from_index);
    DELETE FROM session_time_repairs
    WHERE federation_id = job.federation_id AND block_height = job.block_height;
    RETURN TRUE;
END;
$$ LANGUAGE plpgsql;
