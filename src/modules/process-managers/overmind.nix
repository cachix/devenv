{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.overmind;
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
  };

  config = lib.mkIf cfg.enable {
    process.manager.args = {
      "root" = config.env.DEVENV_ROOT;
      "socket" = "${config.devenv.runtime}/overmind.sock";
      "procfile" = config.procfile;
    };

    process.manager.command = lib.mkDefault ''
      OVERMIND_ENV=${config.procfileEnv} ${cfg.package}/bin/overmind start \
        ${lib.cli.toGNUCommandLineShell {} config.process.manager.args} \
        "$@" &
    '';

    packages = [ cfg.package ];
  };
}
