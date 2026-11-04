set -e

wait_for_processes
test "$(psql postgres -tAc 'SHOW max_connections')" = "42"
