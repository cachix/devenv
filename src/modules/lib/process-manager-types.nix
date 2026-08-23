{ lib }:
let
  types = lib.types;
in
{
  capabilities = types.submodule {
    options = {
      background_start = lib.mkOption {
        type = types.bool;
        description = "Whether the manager can remain running after the launching client exits.";
      };
      devenv_attach = lib.mkOption {
        type = types.bool;
        description = "Whether devenv can attach its interactive client to an existing manager.";
      };
      wait_ready = lib.mkOption {
        type = types.bool;
        description = "Whether devenv can wait for process readiness through the manager.";
      };
      individual_control = lib.mkOption {
        type = types.bool;
        description = "Whether devenv can start, stop, and restart individual processes through the manager.";
      };
      cold_start_subset = lib.mkOption {
        type = types.bool;
        description = "Whether the manager can initially start a named subset of processes.";
      };
    };
  };

  adapter = types.submodule {
    options = {
      terminal = lib.mkOption {
        type = types.enum [ "none" "controlling" ];
        description = "Terminal required by the manager launcher.";
      };
      stop = lib.mkOption {
        type = types.enum [ "native-api" "command" "process-scope" ];
        description = "Adapter used to stop the running manager.";
      };
      client = lib.mkOption {
        type = types.enum [ "none" "native-api" ];
        description = "Client protocol used for attach, readiness, and individual process control.";
      };
    };
  };

  stopCommand = types.nullOr types.package;
}
