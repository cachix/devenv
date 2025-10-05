#!/usr/bin/env bash

set -euo pipefail

run_case() {
  local dir="$1"
  pushd "$dir" > /dev/null
  devenv test
  popd > /dev/null
}

run_case project-a
run_case project-b
run_case deep/level1/level2
run_case mixed-imports
