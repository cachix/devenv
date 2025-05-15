set -e

wait_for_port 2345
pg_isready -d template1

# check whether the pg_uuidv7 extension is installed for the testdb database
psql \
    --set ON_ERROR_STOP=on \
    --dbname=testdb \
    --tuples-only \
    --command="SELECT extname FROM pg_extension WHERE extname = 'pg_uuidv7';" \
    | grep -qw pg_uuidv7

# but testdb2 should not have it
psql \
    --set ON_ERROR_STOP=on \
    --dbname=testdb2 \
    --tuples-only \
    --command="SELECT extname FROM pg_extension WHERE extname = 'pg_uuidv7';" \
    | grep -q pg_uuidv7 && exit 1 || true
