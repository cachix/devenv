{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.overmind;
  processManagerTypes = import ../lib/process-manager-types.nix { inherit lib; };
in
{
  options.process.managers.overmind = {
    enable = lib.mkEnableOption "overmind as the process manager" // {
      internal = true;
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.overmind;
      defaultText = lib.literalExpression "pkgs.overmind";
      description = "The overmind package to use.";
    };

    capabilities = lib.mkOption {
      type = processManagerTypes.capabilities;
      internal = true;
      readOnly = true;
      description = "Capabilities provided by the overmind process manager.";
      default = {
        background_start = true;
        devenv_attach = false;
        wait_ready = false;
        individual_control = false;
        cold_start_subset = true;
      };
    };

    adapter = lib.mkOption {
      type = processManagerTypes.adapter;
      internal = true;
      readOnly = true;
      default = { terminal = "none"; stop = "command"; client = "none"; };
      description = "Runtime adapter settings of the overmind process manager.";
    };

    stopCommand = lib.mkOption {
      type = processManagerTypes.stopCommand;
      internal = true;
      readOnly = true;
      description = "Manager-specific graceful stop command, if one is available.";
      default = pkgs.writeShellScript "devenv-overmind-shutdown" ''
        exec ${lib.getExe cfg.package} quit \
          --socket ${lib.escapeShellArg "${config.devenv.runtime}/overmind.sock"}
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    process.manager.args = {
      "root" = config.devenv.root;
      "socket" = "${config.devenv.runtime}/overmind.sock";
      "procfile" = config.procfile;
    };

    process.manager.command = lib.mkDefault ''
      ${lib.getExe cfg.package} start \
        ${(lib.cli.toCommandLineShellGNU or lib.cli.toGNUCommandLineShell) {} config.process.manager.args} \
        "$@" &
    '';

    packages = [ cfg.package ];
  };
}
