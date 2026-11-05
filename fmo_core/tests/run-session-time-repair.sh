#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
db="fmo_repair_test_$$"
fresh_db="${db}_fresh"
createdb "$db"
trap 'dropdb --if-exists "$db"; dropdb --if-exists "$fresh_db"' EXIT
for version in {0..11}; do
    psql -X -q -v ON_ERROR_STOP=1 -1 -d "$db" -f "$root/fmo_core/schema/v${version}.sql" >/dev/null
done
# Populate v11, then check that later migrations preserve counters and assets.
psql -X -q -v ON_ERROR_STOP=1 -1 -d "$db" >/dev/null <<'SQL'
INSERT INTO federations VALUES ('\x03', '\x00');
INSERT INTO sessions VALUES ('\x03', 0, '\x00');
INSERT INTO transactions VALUES ('\x04', '\x03', 0, 0, '\x00');
INSERT INTO transaction_inputs (federation_id, txid, in_index, kind, amount_msat)
VALUES ('\x03', '\x04', 0, 'wallet', 1000);
INSERT INTO transaction_outputs (federation_id, txid, out_index, kind, amount_msat)
VALUES ('\x03', '\x04', 0, 'wallet', 400);
SQL
psql -X -q -v ON_ERROR_STOP=1 -1 -d "$db" -f "$root/fmo_core/schema/v12.sql" >/dev/null
# Simulate an earlier PR build that already applied v13 without the repair.
sed '/-- BEGIN SESSION TIME REPAIR/,$d' "$root/fmo_core/schema/v13.sql" |
    psql -X -q -v ON_ERROR_STOP=1 -1 -d "$db" >/dev/null
# Populate a v13 database before upgrading, including stale derived history.
psql -X -q -v ON_ERROR_STOP=1 -1 -d "$db" >/dev/null <<'SQL'
INSERT INTO federations VALUES ('\x02', '\x00');
INSERT INTO sessions VALUES ('\x02', 0, '\x00');
INSERT INTO transactions VALUES ('\x03', '\x02', 0, 0, '\x00');
INSERT INTO block_times VALUES (12344, '2024-01-01');
INSERT INTO block_height_votes VALUES ('\x02', 0, 0, 0, 12344);
SELECT record_new_session('\x02', 0);
UPDATE session_times SET estimated_session_timestamp = NULL WHERE federation_id = '\x02';
DELETE FROM federation_daily_activity WHERE federation_id = '\x02';
SQL
# Apply the same v13 section that the collector uses for an existing v13 DB.
sed -n '/-- BEGIN SESSION TIME REPAIR/,$p' "$root/fmo_core/schema/v13.sql" |
    psql -X -q -v ON_ERROR_STOP=1 -1 -d "$db" >/dev/null
psql -X -q -v ON_ERROR_STOP=1 -1 -d "$db" >/dev/null <<'SQL'
DO $$ BEGIN
    IF NOT process_session_time_repair() OR NOT process_session_time_repair() THEN RAISE EXCEPTION 'upgrade repairs were not scheduled'; END IF;
    IF NOT EXISTS (SELECT FROM session_times WHERE federation_id = '\x02' AND estimated_session_timestamp = '2024-01-01'::timestamp) THEN RAISE EXCEPTION 'upgrade did not repair stale time'; END IF;
    IF NOT EXISTS (SELECT FROM federation_daily_activity WHERE federation_id = '\x02' AND day = '2024-01-01' AND tx_count = 1) THEN RAISE EXCEPTION 'upgrade did not repair activity'; END IF;
    IF (SELECT session_count FROM federation_session_counts WHERE federation_id = '\x02') <> 1 THEN RAISE EXCEPTION 'upgrade changed session count'; END IF;
    IF NOT EXISTS (SELECT FROM federation_session_counts WHERE federation_id = '\x03' AND session_count = 1) THEN RAISE EXCEPTION 'v11 session count changed'; END IF;
    IF NOT EXISTS (SELECT FROM federation_totals WHERE federation_id = '\x03' AND tx_count = 1 AND tx_volume_msat = 1000 AND wallet_in_msat = 1000 AND wallet_out_msat = 400) THEN RAISE EXCEPTION 'v11 transaction count or wallet totals changed'; END IF;
END $$;
SQL
psql -X -q -v ON_ERROR_STOP=1 -1 -d "$db" -f "$root/fmo_core/tests/session_time_repair.sql" >/dev/null
# Also exercise a fresh database using v13 as a single migration.
createdb "$fresh_db"
for version in {0..13}; do
    psql -X -q -v ON_ERROR_STOP=1 -1 -d "$fresh_db" -f "$root/fmo_core/schema/v${version}.sql" >/dev/null
done
psql -X -q -v ON_ERROR_STOP=1 -1 -d "$fresh_db" -f "$root/fmo_core/tests/session_time_repair.sql" >/dev/null
echo 'Session timestamp repair regression passed.'
