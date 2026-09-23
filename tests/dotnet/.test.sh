#!/usr/bin/env bash
set -euo pipefail

export LD_LIBRARY_PATH=/tmp/devenv-dotnet-probe

devenv shell -- bash -euo pipefail -c '
  [[ "$LD_LIBRARY_PATH" == /tmp/devenv-dotnet-probe ]]
  dotnet --info > /dev/null
  dotnet restore Globalization.csproj --ignore-failed-sources -p:NuGetAudit=false > /dev/null
  [[ "$(dotnet run --project Globalization.csproj --no-restore)" == "İ" ]]
'
