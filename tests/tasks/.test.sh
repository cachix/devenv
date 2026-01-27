#!/usr/bin/env bash

set -xe

# Cleanup function
cleanup() {
  rm -f test-basic.txt cwd-test.txt python-output.txt should-not-exist.txt input-result.json
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

# Test: showOutput option displays output
OUTPUT=$(devenv tasks run test:with-output 2>&1)
if ! echo "$OUTPUT" | grep -q "VISIBLE_OUTPUT_MARKER"; then
  echo "FAIL: test:with-output should show output but didn't"
  echo "Got output: $OUTPUT"
  exit 1
fi
echo "✓ showOutput=true displays output"

# Test: without showOutput hides output
OUTPUT=$(devenv tasks run test:without-output 2>&1)
if echo "$OUTPUT" | grep -q "HIDDEN_OUTPUT_MARKER"; then
  echo "FAIL: test:without-output should hide output but didn't"
  echo "Got output: $OUTPUT"
  exit 1
fi
echo "✓ showOutput=false hides output"

# Test: --show-output flag overrides and shows output
OUTPUT=$(devenv tasks run test:without-output --show-output 2>&1)
if ! echo "$OUTPUT" | grep -q "HIDDEN_OUTPUT_MARKER"; then
  echo "FAIL: --show-output flag should show output but didn't"
  echo "Got output: $OUTPUT"
  exit 1
fi
echo "✓ --show-output flag displays output"

# Test: Nix-defined task input is passed via DEVENV_TASK_INPUT
devenv tasks run test:input
INPUT=$(cat input-result.json)
if ! echo "$INPUT" | grep -q '"greeting":"hello"'; then
  echo "FAIL: Expected input to contain greeting=hello"
  echo "Got: $INPUT"
  exit 1
fi
if ! echo "$INPUT" | grep -q '"count":1'; then
  echo "FAIL: Expected input to contain count=1"
  echo "Got: $INPUT"
  exit 1
fi
echo "✓ Nix-defined task input works"

# Test: --input flag overrides existing input
devenv tasks run test:input --input count=5
INPUT=$(cat input-result.json)
if ! echo "$INPUT" | grep -q '"count":5'; then
  echo "FAIL: Expected --input to override count to 5"
  echo "Got: $INPUT"
  exit 1
fi
if ! echo "$INPUT" | grep -q '"greeting":"hello"'; then
  echo "FAIL: Expected greeting to remain after --input override"
  echo "Got: $INPUT"
  exit 1
fi
echo "✓ --input flag overrides existing input"

# Test: --input flag adds new keys
devenv tasks run test:input --input extra=new
INPUT=$(cat input-result.json)
if ! echo "$INPUT" | grep -q '"extra":"new"'; then
  echo "FAIL: Expected --input to add extra=new"
  echo "Got: $INPUT"
  exit 1
fi
echo "✓ --input flag adds new keys"

# Test: --input-json flag overrides input
devenv tasks run test:input --input-json '{"count":99,"added":true}'
INPUT=$(cat input-result.json)
if ! echo "$INPUT" | grep -q '"count":99'; then
  echo "FAIL: Expected --input-json to set count=99"
  echo "Got: $INPUT"
  exit 1
fi
if ! echo "$INPUT" | grep -q '"added":true'; then
  echo "FAIL: Expected --input-json to add added=true"
  echo "Got: $INPUT"
  exit 1
fi
echo "✓ --input-json flag works"

echo ""
echo "All task tests passed!"
