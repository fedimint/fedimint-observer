-- Run against a freshly migrated disposable database.
INSERT INTO federations VALUES ('\x01', '\x00');
INSERT INTO sessions VALUES ('\x01', 0, '\x00'), ('\x01', 1, '\x00');
INSERT INTO transactions VALUES ('\x01', '\x01', 0, 0, '\x00'), ('\x02', '\x01', 1, 0, '\x00');
INSERT INTO transaction_inputs (federation_id, txid, in_index, kind, amount_msat)
VALUES ('\x01', '\x01', 0, 'mint', 100), ('\x01', '\x02', 0, 'mint', 200);
-- Both sessions exist before their selected block timestamp.
INSERT INTO block_height_votes VALUES ('\x01', 0, 0, 0, 12345);
SELECT record_new_session('\x01', 0);
SELECT record_new_session('\x01', 1);
DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM session_time_repairs WHERE federation_id = '\x01' AND block_height = 12345 AND from_index = 0) THEN
        RAISE EXCEPTION 'missing timestamp was not queued';
    END IF;
    IF process_session_time_repair() THEN RAISE EXCEPTION 'repaired before timestamp arrived'; END IF;
    IF EXISTS (SELECT FROM federation_daily_activity WHERE federation_id = '\x01') THEN
        RAISE EXCEPTION 'undated activity unexpectedly counted';
    END IF;
END $$;
INSERT INTO block_times VALUES (12345, '2024-01-02 12:00:00');
-- An interrupted repair rolls back both its changes and removal of the job.
SAVEPOINT before_repair;
SELECT process_session_time_repair();
ROLLBACK TO SAVEPOINT before_repair;
DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM session_time_repairs WHERE federation_id = '\x01' AND block_height = 12345) THEN
        RAISE EXCEPTION 'rollback lost the repair';
    END IF;
END $$;
DO $$ BEGIN
    IF NOT process_session_time_repair() THEN RAISE EXCEPTION 'ready repair was not processed'; END IF;
    IF EXISTS (SELECT FROM session_times WHERE federation_id = '\x01' AND estimated_session_timestamp IS DISTINCT FROM '2024-01-02 12:00:00'::timestamp) THEN
        RAISE EXCEPTION 'session timestamps were not repaired';
    END IF;
    IF NOT EXISTS (SELECT FROM federation_daily_activity WHERE federation_id = '\x01' AND day = '2024-01-02' AND tx_count = 2 AND funded_tx_count = 2 AND volume_msat = 300) THEN
        RAISE EXCEPTION 'daily activity was not repaired';
    END IF;
    IF EXISTS (SELECT FROM session_time_repairs WHERE federation_id = '\x01') THEN
        RAISE EXCEPTION 'completed repair remains queued';
    END IF;
END $$;
