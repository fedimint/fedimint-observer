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
