-- Run against a disposable database migrated to the latest version, in one
-- transaction (psql -1). Uses federations \x05 and \x06 only.
\ir repair_worker.sql
INSERT INTO federations VALUES ('\x05', '\x00'), ('\x06', '\x00');
INSERT INTO block_times VALUES (20001, '2024-03-01 10:00'), (20002, '2024-03-02 10:00'),
                               (20003, '2024-04-15 10:00')
ON CONFLICT DO NOTHING;
INSERT INTO sessions SELECT '\x05', i, '\x00' FROM generate_series(0, 5) i;
INSERT INTO block_height_votes VALUES ('\x05', 0, 0, 0, 20001), ('\x05', 2, 0, 0, 20002),
                                      ('\x05', 4, 0, 0, 20003);
INSERT INTO transactions
SELECT '\x50'::bytea || decode(lpad(to_hex(i), 2, '0'), 'hex'), '\x05', i, 1, '\x00'
FROM generate_series(0, 5) i;
INSERT INTO transaction_inputs (federation_id, txid, in_index, kind, amount_msat)
SELECT '\x05', '\x50'::bytea || decode(lpad(to_hex(i), 2, '0'), 'hex'), 0, 'mint', 100 * (i + 1)
FROM generate_series(0, 5) i;
INSERT INTO transaction_outputs (federation_id, txid, out_index, kind, amount_msat)
SELECT '\x05', '\x50'::bytea || decode(lpad(to_hex(i), 2, '0'), 'hex'), 0, 'wallet', NULL
FROM generate_series(0, 5) i;
SELECT record_new_session('\x05', i) FROM generate_series(0, 5) i;

-- Totals triggers run at commit; this runs them now
SET CONSTRAINTS ALL IMMEDIATE;
DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM federation_totals WHERE federation_id = '\x05'
                   AND tx_count = 6 AND tx_volume_msat = 2100 AND wallet_out_msat = 0) THEN
        RAISE EXCEPTION 'deferred totals triggers did not count the inserts';
    END IF;
END $$;
-- A projector completing an amount moves the totals by the difference
UPDATE transaction_outputs SET amount_msat = 50 WHERE federation_id = '\x05';
SET CONSTRAINTS ALL IMMEDIATE;
DO $$ BEGIN
    IF (SELECT wallet_out_msat FROM federation_totals WHERE federation_id = '\x05') <> 300 THEN
        RAISE EXCEPTION 'deferred update trigger did not apply the delta';
    END IF;
END $$;

CREATE TEMP TABLE expected AS
SELECT day, tx_count, funded_tx_count, volume_msat FROM federation_daily_activity
WHERE federation_id = '\x05';
DO $$ BEGIN
    IF (SELECT count(*) FROM expected) <> 3 THEN RAISE EXCEPTION 'expected three active days'; END IF;
END $$;

-- A rebuild from a session rewrites its times and only the days from there on
UPDATE session_times SET estimated_session_timestamp = NULL
WHERE federation_id = '\x05' AND session_index >= 3;
UPDATE federation_daily_activity SET tx_count = 99
WHERE federation_id = '\x05' AND day >= '2024-03-02';
SELECT pg_temp.rebuild_read_models('\x05', 3);
DO $$ BEGIN
    IF EXISTS (SELECT day, tx_count, funded_tx_count, volume_msat FROM federation_daily_activity
               WHERE federation_id = '\x05'
               EXCEPT SELECT * FROM expected)
       OR (SELECT count(*) FROM federation_daily_activity WHERE federation_id = '\x05') <> 3 THEN
        RAISE EXCEPTION 'rebuild from a session did not restore the activity';
    END IF;
END $$;

-- Recounting in day ranges, as the collector's steps do, equals the full count
DELETE FROM federation_daily_activity WHERE federation_id = '\x05';
SELECT rebuild_daily_activity_days('\x05', '2024-03-01', '2024-03-02');
SELECT rebuild_daily_activity_days('\x05', '2024-03-02', '2024-04-01');
SELECT rebuild_daily_activity_days('\x05', '2024-04-01', NULL);
DO $$ BEGIN
    IF EXISTS (SELECT day, tx_count, funded_tx_count, volume_msat FROM federation_daily_activity
               WHERE federation_id = '\x05'
               EXCEPT SELECT * FROM expected)
       OR (SELECT count(*) FROM federation_daily_activity WHERE federation_id = '\x05') <> 3 THEN
        RAISE EXCEPTION 'stepped activity rebuild differs from the full one';
    END IF;
    IF session_activity_floor('\x05', 3) <> '2024-03-02' THEN
        RAISE EXCEPTION 'wrong first day of the sessions from 3';
    END IF;
END $$;

-- Repair jobs: scheduling merges, a claim covers what was ready when it was
-- made, and claims rotate between federations
DELETE FROM session_time_repairs;
SELECT schedule_read_model_rebuild('\x05', 3);
SELECT schedule_read_model_rebuild('\x05', 1);
SELECT schedule_read_model_rebuild('\x06', 0);
DO $$
DECLARE
    first RECORD;
    second RECORD;
    third RECORD;
BEGIN
    IF (SELECT from_index FROM session_time_repairs WHERE federation_id = '\x05') <> 1 THEN
        RAISE EXCEPTION 'scheduling did not keep the earliest session';
    END IF;
    SELECT * INTO first FROM claim_session_time_repair();
    -- The claimed federation is attempted again only after the other one
    SELECT * INTO second FROM claim_session_time_repair();
    IF first.fed = second.fed THEN RAISE EXCEPTION 'claims did not rotate'; END IF;
    -- Scheduled again while claimed: the claim's finish must keep it
    PERFORM schedule_read_model_rebuild(first.fed, 2);
    PERFORM finish_session_time_repair(first.fed, first.start_index, first.claimed_at);
    PERFORM finish_session_time_repair(second.fed, second.start_index, second.claimed_at);
    IF NOT EXISTS (SELECT FROM session_time_repairs WHERE federation_id = first.fed) THEN
        RAISE EXCEPTION 'a job scheduled during its claim was lost';
    END IF;
    IF EXISTS (SELECT FROM session_time_repairs WHERE federation_id = second.fed) THEN
        RAISE EXCEPTION 'a finished job remains queued';
    END IF;
    SELECT * INTO third FROM claim_session_time_repair();
    IF third.fed IS DISTINCT FROM first.fed THEN RAISE EXCEPTION 'rescheduled job not claimable'; END IF;
    PERFORM finish_session_time_repair(third.fed, third.start_index, third.claimed_at);
    SELECT * INTO third FROM claim_session_time_repair();
    IF third.fed IS NOT NULL THEN RAISE EXCEPTION 'empty queue returned a claim'; END IF;
END $$;
