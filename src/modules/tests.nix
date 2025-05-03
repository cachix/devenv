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
            echo "• Waiting for process-compose processes to be ready (timeout: $timeout seconds)..."

            # TODO(sander): Update this to use the new wait command once it's available in process-compose
            timeout $timeout bash -c '
              while true; do
                output=$(${lib.getExe config.process.managers.process-compose.package} list --output json 2>/dev/null)
                if [ $? -eq 0 ]; then
                  not_ready=$(echo "$output" | ${lib.getExe pkgs.jq} -r ".[] | select(.is_ready == \"Not Ready\" ) | .name" 2>/dev/null)
                  if [ -z "$not_ready" ]; then
                    echo "✓ All processes are ready"
                    exit 0
                  else
                    echo "• Waiting for processes to become ready: $not_ready"
                  fi
                else
                  echo "• Waiting for process-compose to be ready..."
                fi
                sleep 2
              done
            '
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
