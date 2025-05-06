{ pkgs, config, lib, ... }:

{
  options = {
    enterTest = lib.mkOption {
      type = lib.types.lines;
      description = "Bash code to execute to run the test.";
    };

    devenv.isTesting = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Whether the environment is being used for testing.";
    };

    test = lib.mkOption {
      type = lib.types.package;
      internal = true;
      default = pkgs.writeShellScript "devenv-test" ''
        set -euo pipefail
        echo "• Testing ..."
        ${config.enterTest}
      '';
    };
  };

  config = {
    enterTest = ''
      # Wait for the port to be open until the timeout is reached
      wait_for_port() {
        local port=$1
        local timeout=''${2:-15}

        timeout $timeout bash -c "until ${pkgs.libressl.nc}/bin/nc -z localhost $port 2>/dev/null; do sleep 0.5; done"
      }

      # Wait for processes to be healthy
      wait_for_processes() {
        local timeout=''${1:-120}

        case "${config.process.manager.implementation}" in
          "process-compose")
            echo "• Waiting for process-compose processes to be ready (timeout: $timeout seconds)..." >&2

            # TODO(sander): Update this to use the new wait command once it's available in process-compose
            # Pre-calculate which processes have readiness probes
            readiness_probes='${builtins.toJSON (lib.mapAttrs (name: proc:
              lib.hasAttrByPath ["process-compose" "readiness_probe"] proc
            ) config.processes)}'

            # Use a simple shell script that handles a single task
            process_compose_wait=${pkgs.writeShellScript "process-compose-wait" ''
              # Helper function to print messages to stderr
              log() {
                echo "$@" >&2
              }

              # Check if a process has a readiness probe
              has_readiness_probe() {
                local process_name="$1"
                echo "$readiness_probes" | ${lib.getExe pkgs.jq} -r --arg name "$process_name" '.[$name] // false'
              }

              while true; do
                # Get the process list with error handling
                output=$(${lib.getExe config.process.managers.process-compose.package} list --output json 2>/dev/null)
                if [ $? -ne 0 ]; then
                  log "• Waiting for process-compose to be ready..."
                  sleep 2
                  continue
                fi

                # Extract processes by status
                pending=$(echo "$output" | ${lib.getExe pkgs.jq} -r '[.[] | select(.status == "Pending") | .name] | join(" ")')
                not_ready=$(echo "$output" | ${lib.getExe pkgs.jq} -r '[.[] | select(.status == "Running" and .is_ready != "Ready") | .name] | join(" ")')
                failed=$(echo "$output" | ${lib.getExe pkgs.jq} -r '[.[] | select(.status == "Exited" and .exit_code != 0) | .name] | join(" ")')

                # Check for failed processes and warn about them
                if [ -n "$failed" ]; then
                  log "Warning: Some processes have failed: $failed"
                fi

                # Filter not_ready processes that have readiness probes
                filtered_not_ready=""
                if [ -n "$not_ready" ]; then
                  for proc in $not_ready; do
                    if [ "$(has_readiness_probe "$proc")" = "true" ]; then
                      if [ -n "$filtered_not_ready" ]; then
                        filtered_not_ready="$filtered_not_ready $proc"
                      else
                        filtered_not_ready="$proc"
                      fi
                    fi
                  done
                fi

                # Combine processes we're waiting for
                waiting_for="$pending"
                if [ -n "$filtered_not_ready" ]; then
                  if [ -n "$waiting_for" ]; then
                    waiting_for="$waiting_for $filtered_not_ready"
                  else
                    waiting_for="$filtered_not_ready"
                  fi
                fi

                if [ -z "$waiting_for" ]; then
                  log "✓ All processes are ready"
                  exit 0
                else
                  # Show detailed status
                  msg="• Waiting for processes to become ready:"
                  if [ -n "$pending" ]; then
                    msg="$msg Pending:[$pending]"
                  fi
                  if [ -n "$filtered_not_ready" ]; then
                    msg="$msg Not Ready:[$filtered_not_ready]"
                  fi
                  log "$msg"
                fi

                sleep 2
              done
            ''}

            timeout $timeout $process_compose_wait
            ;;
          "")
            # No process manager configured, nothing to wait for
            ;;
          *)
            echo "✗ Unsupported process manager implementation: ${config.process.manager.implementation}" >&2
            echo "✗ wait_for_processes is only implemented for process-compose" >&2
            return 1
            ;;
        esac
      }

      export -f wait_for_port
      export -f wait_for_processes

      if [ -f ./.test.sh ]; then
        echo "• Running .test.sh..."
        ./.test.sh
      fi
    '';
  };
}
