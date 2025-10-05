#!/usr/bin/env bash
set -e

echo "Testing processes.pid protection..."

# Clean up any stale processes from previous runs
devenv processes down 2>/dev/null || true

# Test 1: Run devenv up -d once
echo "✓ Starting processes in background..."
devenv up -d

# Give it a moment to start
sleep 2

# Test 2: Try running devenv up -d again - should fail
echo "✓ Attempting to start processes again (should fail)..."
if devenv up -d 2>&1 | grep -q "Processes are already running"; then
    echo "✓ Second invocation correctly prevented"
else
    echo "✗ Second invocation should have been prevented"
    devenv processes down
    exit 1
fi

# Test 3: Clean up
echo "✓ Stopping processes..."
devenv processes down

# Test 4: Verify we can start again after stopping
echo "✓ Starting processes after clean stop..."
devenv up -d
sleep 1

# Clean up
devenv processes down
