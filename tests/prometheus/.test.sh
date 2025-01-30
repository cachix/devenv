set -e

trap 'rm -rf "${DEVENV_STATE}/prometheus"' EXIT

wait_for_port 9090

# Test the API endpoints
curl -sf http://localhost:9090/-/ready
curl -sf http://localhost:9090/-/healthy

# Test basic query functionality
response=$(curl -sf 'http://localhost:9090/api/v1/query?query=up')
if ! echo "$response" | grep -q '"status":"success"'; then
  echo "Query test failed"
  exit 1
fi

# Test our ping script
ping-prometheus
