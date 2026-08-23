{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.hivemind;
  processManagerTypes = import ../lib/process-manager-types.nix { inherit lib; };
in
{
  options.process.managers.hivemind = {
    enable = lib.mkEnableOption "hivemind as the process manager" // {
      internal = true;
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.hivemind;
      defaultText = lib.literalExpression "pkgs.hivemind";
      description = "The hivemind package to use.";
    };

    capabilities = lib.mkOption {
      type = processManagerTypes.capabilities;
      internal = true;
      readOnly = true;
      description = "Capabilities provided by the hivemind process manager.";
      default = {
        background_start = true;
        devenv_attach = false;
        wait_ready = false;
        individual_control = false;
        cold_start_subset = false;
      };
    };

    adapter = lib.mkOption {
      type = processManagerTypes.adapter;
      internal = true;
      readOnly = true;
      default = { terminal = "none"; stop = "process-scope"; client = "none"; };
      description = "Runtime adapter settings of the hivemind process manager.";
    };

    stopCommand = lib.mkOption {
      type = processManagerTypes.stopCommand;
      internal = true;
      readOnly = true;
      default = null;
      description = "Manager-specific graceful stop command, if one is available.";
    };
  };

  config = lib.mkIf cfg.enable {
    process.manager.args = {
      "print-timestamps" = true;
    };

    process.manager.command = lib.mkDefault ''
      ${lib.getExe cfg.package} \
        ${(lib.cli.toCommandLineShellGNU or lib.cli.toGNUCommandLineShell) {} config.process.manager.args} \
        "$@" ${config.procfile} &
    '';

    packages = [ cfg.package ];
  };
}
