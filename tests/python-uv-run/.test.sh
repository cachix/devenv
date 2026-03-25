#!/usr/bin/env bash
set -exu

# Verify that `uv run python` sees libraries from languages.python.libraries.
# Regression test for https://github.com/cachix/devenv/issues/2335

if [[ "$(uname)" == "Darwin" ]]; then
  lib_path_var="DYLD_LIBRARY_PATH"
else
  lib_path_var="LD_LIBRARY_PATH"
fi

uv run python -c "import os; assert 'zlib' in os.environ['$lib_path_var'], 'zlib missing from $lib_path_var'"
