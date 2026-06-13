#!/usr/bin/env bash

set -euo pipefail

run_case() {
  local dir="$1"
  pushd "$dir" > /dev/null
  devenv test
  popd > /dev/null
}

run_case conan-toolchain
run_case devenv-toolchain
