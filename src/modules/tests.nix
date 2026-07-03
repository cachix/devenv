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
        ${config.enterTest}
      '';
    };
  };

  config = lib.mkMerge [
    {
      enterTest = lib.mkBefore ''
        # Wait for the port to be open until the timeout is reached
        wait_for_port() {
          local port=$1
          local timeout=''${2:-15}

          if ! timeout "$timeout" bash -c "until ${pkgs.libressl.nc}/bin/nc -z localhost $port 2>/dev/null; do sleep 0.5; done"; then
            echo "Error: Port $port did not become available within $timeout seconds."
            exit 1
          fi
        }

        # Wait for processes to be healthy
        wait_for_processes() {
          local timeout=''${1:-120}

          case "${config.process.manager.implementation}" in
            "process-compose")
              echo "• Waiting for process-compose processes to be ready (timeout: $timeout seconds)..." >&2
              devenv processes wait --timeout $timeout
              echo "✓ All processes are ready" >&2
              ;;
            "native")
              echo "• Waiting for native processes to be ready (timeout: $timeout seconds)..." >&2
              devenv processes wait --timeout $timeout
              echo "✓ All processes are ready" >&2
              ;;
            "")
              # No process manager configured, nothing to wait for
              ;;
            *)
              echo "✗ Unsupported process manager implementation: ${config.process.manager.implementation}" >&2
              return 1
              ;;
          esac
        }

        export -f wait_for_port
        export -f wait_for_processes
      '';
    }
    {
      enterTest = lib.mkAfter ''
        if [ -f ./.test.sh ]; then
          echo "• Running .test.sh..."
          ./.test.sh
        fi
      '';
    }
  ];
}
