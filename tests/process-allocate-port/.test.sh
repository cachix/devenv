#!/usr/bin/env bash

# Test automatic port allocation for processes

set -e

echo "=== Test 1: Auto-allocate distinct ports ==="

# Start processes in detached mode
echo "Starting processes..."
devenv up -d

# Give processes time to start
sleep 2

# Check if processes are running
if [ -f .devenv/processes.pid ]; then
  pid=$(cat .devenv/processes.pid)
  if kill -0 "$pid" 2>/dev/null; then
    echo "Process-compose is running (PID: $pid)"
  else
    echo "ERROR: Process-compose has exited!"
    exit 1
  fi
else
  echo "ERROR: PID file not found"
  exit 1
fi

# Check that both ports are listening
port_count=$(ss -tlnp 2>/dev/null | grep -c python || echo 0)
if [ "$port_count" -ge 2 ]; then
  echo "Both Python servers are listening"
else
  echo "ERROR: Expected 2 servers listening, found $port_count"
  ss -tlnp 2>/dev/null | grep python || true
  exit 1
fi

# Verify ports are distinct
ports=$(ss -tlnp 2>/dev/null | grep python | awk '{print $4}' | grep -oP ':\K[0-9]+$' | sort -u)
port_array=($ports)
if [ "${#port_array[@]}" -ge 2 ]; then
  echo "Servers listening on distinct ports: ${port_array[*]}"
else
  echo "ERROR: Ports are not distinct"
  exit 1
fi

# Clean up first test
echo "Stopping processes..."
devenv processes down
sleep 1

echo ""
echo "=== Test 2: Strict ports mode ==="

# Test --strict-ports with a port that's already in use
echo "Starting external server on port 18080..."
python3 -m http.server 18080 &
external_pid=$!
sleep 1

# Verify external server is running
if ! kill -0 "$external_pid" 2>/dev/null; then
  echo "ERROR: Failed to start external server"
  exit 1
fi
echo "External server running on port 18080 (PID: $external_pid)"

# Try to start devenv with --strict-ports (should fail)
echo "Trying devenv up --strict-ports (should fail)..."
if output=$(devenv up --strict-ports 2>&1); then
  echo "ERROR: devenv up --strict-ports should have failed but succeeded"
  kill "$external_pid" 2>/dev/null || true
  devenv processes down 2>/dev/null || true
  exit 1
fi

# Check that the error message mentions the port being in use
if echo "$output" | grep -qi "already in use"; then
  echo "Got expected 'already in use' error message"
else
  echo "ERROR: Expected 'already in use' error but got:"
  echo "$output" | tail -5
  kill "$external_pid" 2>/dev/null || true
  exit 1
fi

# Clean up external server
kill "$external_pid" 2>/dev/null || true
wait "$external_pid" 2>/dev/null || true

echo ""
echo "All port allocation tests passed!"
