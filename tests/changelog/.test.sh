#!/usr/bin/env bash

set -xe

# Test 1: Verify devenv shell works with changelog module
echo "Testing devenv shell loads without errors..."
devenv shell -- echo "Changelog module loaded" > /dev/null 2>&1
echo "✓ Configuration with changelogs loads successfully"

# Test 2: Run devenv update and capture output
echo "Running devenv update..."
UPDATE_OUTPUT=$(devenv update 2>&1 || true)

echo "Update output (first 500 chars):"
echo "$UPDATE_OUTPUT" | head -c 500
echo ""

# Test 3: Check if changelog header is present (optional - depends on whether devenv was updated)
echo "Checking for changelog in output..."
if echo "$UPDATE_OUTPUT" | grep -q "devenv Changelog"; then
  echo "✓ Changelog header found in update output"

  # If changelog was shown, verify it contains our test entries
  if echo "$UPDATE_OUTPUT" | grep -q "changelog system"; then
    echo "✓ Test changelog entry found in output"
  fi
else
  echo "⚠️  Changelog header not found (may not have been an update to devenv input)"
  echo "This is expected if the devenv input lastModified timestamp did not change."
fi

