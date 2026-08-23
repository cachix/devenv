set -e

. "$DEVENV_TEST_LIB"

wait_until 30 curl -sf --connect-timeout 5 --max-time 5 \
    http://localhost:8083/connectors

curl -sf http://localhost:8083/connectors
