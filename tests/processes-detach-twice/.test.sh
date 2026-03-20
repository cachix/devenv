#!/usr/bin/env bash

# Test that running devenv up -d twice should fail when processes are already running

set -ex

# Start processes in detached mode
devenv up -d

# Wait for the daemon to fully start
devenv processes wait

# Try to start again - this should fail with an error about processes already running
if output=$(devenv up -d 2>&1); then
  echo "✗ Second 'devenv up -d' should have failed but succeeded"
  devenv processes down || true
  exit 1
elif [[ "$output" == *"Processes already running"* ]]; then
  echo "✓ Second 'devenv up -d' correctly detected running processes"
else
  echo "✗ Second 'devenv up -d' failed but with unexpected error: $output"
  devenv processes down || true
  exit 1
fi

# Stop the processes
devenv processes down

# Now we should be able to start processes again
devenv up -d

# Wait for the daemon to fully start
devenv processes wait

# Verify it started successfully by checking that down works
if devenv processes down 2>&1; then
  echo "✓ Processes started successfully after stopping"
else
  echo "✗ Failed to start processes after stopping"
  exit 1
fi
