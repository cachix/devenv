{ pkgs, lib, config, ... }:
# Regression test for https://github.com/cachix/devenv/issues/2879
#
# Under process-compose, a short-lived process (one that runs and exits) used
# to leave the `devenv-tasks` wrapper lingering — process-compose kept
# reporting it as Running indefinitely. With `--supervisor external` and
# `--on-idle exit` plumbed in, devenv-tasks runs the process once and exits
# when it settles, letting process-compose mark it Completed.
let
  pcBin = lib.getExe config.process.managers.process-compose.package;
in
{
  process.manager.implementation = "process-compose";

  packages = [ pkgs.jq ];

  # Short-lived process: runs immediately, exits 0.
  processes.shortlived = {
    exec = "echo hello-from-shortlived";
  };

  # Long-lived process kept around so process-compose itself stays up while
  # we assert on the short-lived one's status.
  processes.keepalive = {
    exec = "sleep 60";
  };

  enterTest = ''
    set -euo pipefail

    # Poll process-compose's typed `process get` API until shortlived settles.
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
