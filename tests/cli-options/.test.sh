#!/usr/bin/env bash
set -eu -o pipefail

# Test CLI options feature
cd "$(dirname "$0")"

# Test basic string and bool types
OUTPUT=$(devenv --option languages.rust.channel:string beta --option services.redis.enable:bool true info)

# Check for expected RUST_VERSION
echo "$OUTPUT" | grep -E "RUST_VERSION:.*beta" || {
  echo "ERROR: Expected CLI option override for RUST_VERSION to be 'beta'"
  echo $OUTPUT
  exit 1
}

# Check for expected REDIS_ENABLED
echo "$OUTPUT" | grep -E "REDIS_ENABLED:.*1" || {
  echo "ERROR: Expected CLI option override for REDIS_ENABLED to be '1'"
  echo $OUTPUT
  exit 1
}

# Test int type
OUTPUT=$(devenv --option env.TEST_INT:int 42 info)
echo "$OUTPUT" | grep -E "TEST_INT:.*42" || {
  echo "ERROR: Expected CLI option override for TEST_INT to be '42'"
  echo $OUTPUT
  exit 1
}

# Test float type
OUTPUT=$(devenv --option env.TEST_FLOAT:float 3.14 info)
echo "$OUTPUT" | grep -E "TEST_FLOAT:.*3.14" || {
  echo "ERROR: Expected CLI option override for TEST_FLOAT to be '3.14'"
  echo $OUTPUT
  exit 1
}

# Test path type
OUTPUT=$(devenv --option env.TEST_PATH:path somepath info)
echo "$OUTPUT" | grep -E "TEST_PATH:.*/somepath" || {
  echo "ERROR: Expected CLI option override for TEST_PATH to include 'somepath'"
  echo $OUTPUT
  exit 1
}

# Test pkgs type
CMD_OUTPUT=$(devenv --option packages:pkgs "hello cowsay" shell which hello)
if [ $? -ne 0 ]; then
  echo "ERROR: Expected 'hello' package to be available in shell via pkgs type"
  exit 1
fi

# Test if cowsay is also available
CMD_OUTPUT=$(devenv --option packages:pkgs "hello cowsay" shell which cowsay)
if [ $? -ne 0 ]; then
  echo "ERROR: Expected 'cowsay' package to be available in shell via pkgs type"
  exit 1
fi

# Test invalid type (should fail)
if devenv --option languages.rust.version:invalid value info &> /dev/null; then
  echo "ERROR: Expected CLI option with invalid type to fail"
  echo $OUTPUT
  exit 1
fi

# Test missing type (should fail)
if devenv --option languages.rust.version value info &> /dev/null; then
  echo "ERROR: Expected CLI option without type specification to fail"
  echo $OUTPUT
  exit 1
fi
