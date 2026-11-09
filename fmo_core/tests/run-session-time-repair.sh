#!/usr/bin/env bash
# Upgrade and fresh-setup regression for the core schema and its read-model
# repairs. Creates and drops its own databases on the server libpq's PG*
# variables point at.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
db="fmo_repair_test_$$"
fresh_db="${db}_fresh"
createdb "$db"
trap 'dropdb --if-exists "$db"; dropdb --if-exists "$fresh_db"' EXIT
# Every core migration, in order, so new ones are picked up without editing this
versions=$(cd "$root/fmo_core/schema" && ls v*.sql | sed 's/^v//; s/\.sql$//' | sort -n)
migrate() { # database, first version, last version (empty: the latest)
    for version in $versions; do
        if [ "$version" -ge "$2" ] && { [ -z "${3:-}" ] || [ "$version" -le "$3" ]; }; then
            psql -X -q -v ON_ERROR_STOP=1 -1 -d "$1" -f "$root/fmo_core/schema/v${version}.sql" >/dev/null
        fi
    done
}
sql() { psql -X -q -v ON_ERROR_STOP=1 -1 -d "$1" -f "$root/fmo_core/tests/repair_worker.sql" -f - >/dev/null; }

# A populated v11 database, as the last release leaves it
migrate "$db" 0 11
psql -X -q -v ON_ERROR_STOP=1 -1 -d "$db" >/dev/null <<'SQL'
INSERT INTO federations VALUES ('\x03', '\x00');
INSERT INTO sessions VALUES ('\x03', 0, '\x00');
INSERT INTO transactions VALUES ('\x04', '\x03', 0, 0, '\x00');
INSERT INTO transaction_inputs (federation_id, txid, in_index, kind, amount_msat)
VALUES ('\x03', '\x04', 0, 'wallet', 1000);
INSERT INTO transaction_outputs (federation_id, txid, out_index, kind, amount_msat)
VALUES ('\x03', '\x04', 0, 'wallet', 400);
INSERT INTO federations VALUES ('\x02', '\x00');
INSERT INTO sessions VALUES ('\x02', 0, '\x00');
INSERT INTO transactions VALUES ('\x03', '\x02', 0, 0, '\x00');
INSERT INTO block_times VALUES (12344, '2024-01-01');
INSERT INTO block_height_votes VALUES ('\x02', 0, 0, 0, 12344);
SQL
migrate "$db" 12
sql "$db" <<'SQL'
DO $$ BEGIN
    IF EXISTS (SELECT FROM session_time_repairs) THEN RAISE EXCEPTION 'upgrade left repairs: its seeds are complete'; END IF;
    IF pg_temp.process_session_time_repair() THEN RAISE EXCEPTION 'a repair ran without a job'; END IF;
    IF NOT EXISTS (SELECT FROM session_times WHERE federation_id = '\x02' AND estimated_session_timestamp = '2024-01-01'::timestamp) THEN RAISE EXCEPTION 'upgrade did not date the session'; END IF;
    IF NOT EXISTS (SELECT FROM federation_daily_activity WHERE federation_id = '\x02' AND day = '2024-01-01' AND tx_count = 1) THEN RAISE EXCEPTION 'upgrade did not count daily activity'; END IF;
    IF (SELECT session_count FROM federation_session_counts WHERE federation_id = '\x02') <> 1 THEN RAISE EXCEPTION 'upgrade miscounted sessions'; END IF;
    IF NOT EXISTS (SELECT FROM federation_session_counts WHERE federation_id = '\x03' AND session_count = 1) THEN RAISE EXCEPTION 'v11 session count changed'; END IF;
    IF NOT EXISTS (SELECT FROM federation_totals WHERE federation_id = '\x03' AND tx_count = 1 AND tx_volume_msat = 1000 AND wallet_in_msat = 1000 AND wallet_out_msat = 400) THEN RAISE EXCEPTION 'v11 transaction count or wallet totals changed'; END IF;
    IF to_regclass('session_times_v5') IS NOT NULL OR to_regclass('latest_guardian_health') IS NOT NULL
       OR to_regclass('block_height_vote_heights') IS NOT NULL OR to_regclass('federation_sessions') IS NOT NULL THEN
        RAISE EXCEPTION 'upgrade did not drop unused objects';
    END IF;
END $$;
SQL
# A vote for a block without a time yet is repaired once the time arrives
sql "$db" <<'SQL'
INSERT INTO sessions VALUES ('\x02', 1, '\x00');
INSERT INTO block_height_votes VALUES ('\x02', 1, 0, 0, 12399);
SELECT record_new_session('\x02', 1);
DO $$ BEGIN
    IF pg_temp.process_session_time_repair() THEN RAISE EXCEPTION 'repaired before the block time arrived'; END IF;
END $$;
INSERT INTO block_times VALUES (12399, '2024-01-02');
DO $$ BEGIN
    IF NOT pg_temp.process_session_time_repair() THEN RAISE EXCEPTION 'ready repair was not processed'; END IF;
    IF NOT EXISTS (SELECT FROM session_times WHERE federation_id = '\x02' AND session_index = 1 AND estimated_session_timestamp = '2024-01-02'::timestamp) THEN RAISE EXCEPTION 'late block time was not applied'; END IF;
    IF EXISTS (SELECT FROM session_time_repairs) THEN RAISE EXCEPTION 'finished repair remains queued'; END IF;
END $$;
SQL
psql -X -q -v ON_ERROR_STOP=1 -1 -d "$db" -f "$root/fmo_core/tests/session_time_repair.sql" >/dev/null
psql -X -q -v ON_ERROR_STOP=1 -1 -d "$db" -f "$root/fmo_core/tests/read_model_rebuild.sql" >/dev/null
# Also exercise a fresh database through every migration.
createdb "$fresh_db"
migrate "$fresh_db" 0
psql -X -q -v ON_ERROR_STOP=1 -1 -d "$fresh_db" -f "$root/fmo_core/tests/session_time_repair.sql" >/dev/null
psql -X -q -v ON_ERROR_STOP=1 -1 -d "$fresh_db" -f "$root/fmo_core/tests/read_model_rebuild.sql" >/dev/null
echo 'Session timestamp repair regression passed.'
