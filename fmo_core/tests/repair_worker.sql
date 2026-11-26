
CREATE FUNCTION pg_temp.rebuild_read_models(fed BYTEA, from_index INTEGER) RETURNS VOID AS $$
DECLARE
    old_first_day DATE := session_activity_floor(fed, from_index);
    first_day DATE;
BEGIN
    PERFORM update_session_times(fed, from_index, 2147483647);
    first_day := LEAST(old_first_day, session_activity_floor(fed, from_index));
    IF first_day IS NOT NULL THEN
        PERFORM rebuild_daily_activity_days(fed, first_day, NULL);
    END IF;
END;
$$ LANGUAGE plpgsql;
CREATE FUNCTION pg_temp.process_session_time_repair() RETURNS BOOLEAN AS $$
DECLARE
    claim RECORD;
BEGIN
    SELECT * INTO claim FROM claim_session_time_repair();
    IF claim.fed IS NULL THEN
        RETURN FALSE;
    END IF;
    PERFORM pg_temp.rebuild_read_models(claim.fed, claim.start_index);
    PERFORM finish_session_time_repair(claim.fed, claim.start_index, claim.claimed_at);
    RETURN TRUE;
END;
$$ LANGUAGE plpgsql;
