{
  pkgs,
  lib,
  config,
  ...
}:
# #2879: the task wrapper must exit so process-compose reports Completed.
let
  pcBin = lib.getExe config.process.managers.process-compose.package;
in
{
  process.manager.implementation = "process-compose";

  packages = [ pkgs.jq ];

  processes.shortlived = {
    exec = "echo hello-from-shortlived";
  };

  # Keep process-compose alive while the short-lived status is inspected.
  processes.keepalive = {
    exec = "sleep 60";
  };

  enterTest = ''
    set -euo pipefail

    # `process get` returns a one-element array.
    deadline=$((SECONDS + 30))
    while (( SECONDS < deadline )); do
      state=$(${pcBin} process get shortlived --output json 2>/dev/null || echo "[]")
      status=$(echo "$state" | jq -r '.[0].status // empty')
      if [ "$status" = "Completed" ]; then
        echo "✓ shortlived reached Completed"
        break
      fi
      echo "• shortlived status=''${status:-unavailable}, waiting..."
      sleep 1
    done

    state=$(${pcBin} process get shortlived --output json)
    status=$(echo "$state" | jq -r '.[0].status')
    exit_code=$(echo "$state" | jq -r '.[0].exit_code')

    if [ "$status" != "Completed" ]; then
      echo "✗ shortlived status should be Completed, got: $status"
      echo "Full state: $state"
      exit 1
    fi

    if [ "$exit_code" != "0" ]; then
      echo "✗ shortlived exit_code should be 0, got: $exit_code"
      exit 1
    fi

    echo "✓ short-lived process under process-compose exits cleanly (#2879)"
  '';
}
