set -e

PROMETHEUS_PORT=${PROMETHEUS_PORT:?PROMETHEUS_PORT is not set}
PROMETHEUS_URL="http://127.0.0.1:$PROMETHEUS_PORT"

wait_for_port "$PROMETHEUS_PORT"

# Test the API endpoints
curl -sf "$PROMETHEUS_URL/-/ready"
curl -sf "$PROMETHEUS_URL/-/healthy"

# Test basic query functionality
response=$(curl -sf "$PROMETHEUS_URL/api/v1/query?query=up")
if ! echo "$response" | grep -q '"status":"success"'; then
  echo "Query test failed"
  exit 1
fi

# Test our ping script
ping-prometheus
