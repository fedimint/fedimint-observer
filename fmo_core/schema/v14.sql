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

INSERT INTO schema_version(version) VALUES (14);
