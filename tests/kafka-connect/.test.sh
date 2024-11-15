set -e

curl --connect-timeout 5 \
    --max-time 5 \
    --retry 9 \
    --retry-delay 2 \
    --retry-all-errors \
    http://localhost:8083/connectors
