set -e

wait_for_processes
wait_for_port "$PGPORT"
pg_isready -d template1
