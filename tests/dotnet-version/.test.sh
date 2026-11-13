#!/usr/bin/env bash
set -euo pipefail

devenv shell -- bash -euo pipefail -c '
  [[ "$(dotnet --version)" == 9.0.* ]]
'
