set -e

wait_for_processes
wait_for_port 2345
pg_isready -d template1