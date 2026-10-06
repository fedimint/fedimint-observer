pg_start:
  ./scripts/pg_dev/start.sh

pg_stop:
  ./scripts/pg_dev/stop.sh

pg_backup:
  ./scripts/pg_dev/backup.sh

pg_restore BACKUP_FILE:
  ./scripts/pg_dev/restore.sh {{BACKUP_FILE}}

# run the backend server
run *ARGS:
  cargo run -p fmo_server {{ARGS}}

# run the LN gateway prober (see fmo_prober/README.md)
run_prober *ARGS:
  cargo run -p fmo_prober {{ARGS}}

# run the LN gateway prober tests
test_prober:
  cargo test -p fmo_prober
