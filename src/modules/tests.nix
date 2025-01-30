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
        echo "• Setting up shell environment ..."
        ${config.enterShell}

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

      export -f wait_for_port

      if [ -f ./.test.sh ]; then
        echo "• Running .test.sh..."
        ./.test.sh
      fi
    '';
  };
}
