{ pkgs, config, lib, ... }:
let
  cfg = config.process-managers.overmind;
in
{
  options.process-managers.overmind = {
    enable = lib.mkEnableOption "overmind as process-manager";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.overmind;
      defaultText = lib.literalExpression "pkgs.overmind";
      description = "The overmind package to use.";
    };
  };
  config = lib.mkIf cfg.enable {
    processManagerCommand = ''
      OVERMIND_ENV=${config.procfileEnv} ${cfg.package}/bin/overmind start --root ${config.env.DEVENV_ROOT} --procfile ${config.procfile} "$@" &
    '';

    packages = [ cfg.package ];
  };
}
