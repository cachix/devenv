#!/usr/bin/env bash
set -euo pipefail

. "$DEVENV_TEST_LIB"

devenv shell -- python3 test_startup.py "$(devenv_runtime_dir)/processes"
