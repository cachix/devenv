set -e

wait_for_processes
wait_for_port 5555
wait_for_port 6666
pg_isready -d template1

psql \
  --port 6666 \
  --username test \
  --no-password \
  -c '\q'
