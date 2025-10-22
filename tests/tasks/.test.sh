#!/usr/bin/env bash

set -xe

# Cleanup function
cleanup() {
  rm -f test-basic.txt cwd-test.txt python-output.txt should-not-exist.txt
}

trap cleanup EXIT

# Test: Basic task execution
devenv tasks run test:basic-execution
if [ ! -f test-basic.txt ]; then
  echo "FAIL: test-basic.txt does not exist"
  exit 1
fi
echo "✓ Basic execution works"

# Test: Status option prevents execution
devenv tasks run test:status-skip
if [ -f should-not-exist.txt ]; then
  echo "FAIL: should-not-exist.txt exists but status should have prevented execution"
  exit 1
fi
echo "✓ Status skip works"

# Test: Working directory (cwd)
devenv tasks run test:cwd
if [ ! -f cwd-test.txt ]; then
  echo "FAIL: cwd-test.txt not found"
  exit 1
fi

CWD_RESULT=$(cat cwd-test.txt)
CWD_EXPECTED=$(realpath "/tmp")
if [ "$CWD_RESULT" != "$CWD_EXPECTED" ]; then
  echo "FAIL: Expected cwd to be $CWD_EXPECTED but got $CWD_RESULT"
  exit 1
fi
echo "✓ Working directory (cwd) works"

# Test: Task dependencies (before/after)
devenv tasks run test:dep-verify --mode all
echo "✓ Task dependencies work"

# Test: Python package (non-bash runner)
devenv tasks run test:python-success
if [ ! -f python-output.txt ]; then
  echo "FAIL: python-output.txt does not exist"
  exit 1
fi

if ! grep -q "Hello from Python!" python-output.txt; then
  echo "FAIL: Output does not contain expected text"
  cat python-output.txt
  exit 1
fi
echo "✓ Python task execution works"

# Test: Python error handling
if devenv tasks run test:python-error 2>&1; then
  echo "FAIL: test:python-error should have failed but succeeded"
  exit 1
fi
echo "✓ Python error handling works"

echo ""
echo "All task tests passed!"
