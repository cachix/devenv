#/usr/bin/env bash

# Test that running devenv up -d twice should fail when processes are already running

set -ex

# Start processes in detached mode
devenv up -d

# Wait a moment for processes to start
sleep 2

# Try to start again - this should fail with an error about processes already running
if output=$(devenv up -d 2>&1); then
  echo "✗ Second 'devenv up -d' should have failed but succeeded"
  devenv processes down
  exit 1
elif [[ "$output" == *"Processes already running"* ]]; then
  echo "✓ Second 'devenv up -d' correctly detected running processes"
else
  echo "✗ Second 'devenv up -d' failed but with unexpected error: $output"
  devenv processes down
  exit 1
fi

# Stop the processes
devenv processes down

# Wait for processes to fully stop
sleep 2

# Now we should be able to start processes again
devenv up -d

sleep 2

# Verify it started successfully
if [ -f .devenv/processes.pid ]; then
  echo "✓ Processes started successfully after stopping"
else
  echo "✗ Failed to start processes after stopping"
  exit 1
fi

# Clean up
devenv processes down
