{ pkgs, config, lib, ... }:
let
  cfg = config.process-managers.overmind;
in
{
  options.process-managers.overmind = {
    enable = lib.mkOption {
      type = lib.types.bool;
      internal = true;
      default = false;
      description = "Whether to use overmind as the process manager";
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.overmind;
      defaultText = lib.literalExpression "pkgs.overmind";
      description = "The overmind package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    process.manager.args = {
      "root" = config.env.DEVENV_ROOT;
      "socket" = "${config.devenv.runtime}/overmind.sock";
      "procfile" = config.procfile;
    };

    process.manager.command = lib.mkDefault ''
      OVERMIND_ENV=${config.procfileEnv} ${cfg.package}/bin/overmind start \
        ${lib.concatStringsSep " " (lib.cli.toGNUCommandLine {} config.process.manager.args)} \
        "$@" &
    '';

    packages = [ cfg.package ];
  };
}
