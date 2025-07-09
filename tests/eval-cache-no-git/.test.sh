#!/usr/bin/env bash
set -ex

# This test verifies eval cache behavior outside of git repositories
echo "Testing eval cache outside git repo..."

# Run devenv shell to trigger evaluation and caching
echo "Running devenv shell..."
devenv shell echo hello

# Verify that .devenv/input-paths.txt exists and contains our test file
if [ ! -f .devenv/input-paths.txt ]; then
    echo "ERROR: .devenv/input-paths.txt not found!"
    exit 1
fi

echo "Contents of .devenv/input-paths.txt:"
cat .devenv/input-paths.txt

# Check if our test file is tracked in input-paths.txt
if grep -q "test-file.txt" .devenv/input-paths.txt; then
    echo "SUCCESS: test-file.txt found in input-paths.txt"
else
    echo "ERROR: test-file.txt not found in input-paths.txt"
    echo "This suggests the file dependency was not detected"
    exit 1
fi

echo "Test completed successfully!"