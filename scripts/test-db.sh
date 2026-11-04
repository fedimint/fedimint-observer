#!/usr/bin/env bash
# Creates an isolated cluster. Never uses FO_DATABASE or the development database.
set -euo pipefail
cd "$(dirname "$0")/.."
test_state=$(mktemp -d "${TMPDIR:-/tmp}/fmo-db-test.XXXXXX")
cleanup() {
  pg_ctl -D "$test_state/data" -m immediate stop >/dev/null 2>&1 || true
  rm -rf "$test_state"
}
trap cleanup EXIT
initdb -D "$test_state/data" --auth-local=trust --auth-host=reject --no-locale >/dev/null
pg_ctl -D "$test_state/data" -l "$test_state/postgres.log" -o "-k '$test_state' -h '' -p 55439" -w start >/dev/null
createdb -h "$test_state" -p 55439 -U "$(id -un)" fmo_test
export FMO_TEST_DATABASE="host=$test_state port=55439 dbname=fmo_test user=$(id -un)"
cargo test --locked -p fmo_core db_tests -- --ignored --nocapture
