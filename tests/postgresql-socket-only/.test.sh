set -e

wait_for_processes
psql postgres -c "SELECT 1" > /dev/null
