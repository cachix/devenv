{ pkgs, config, lib, ... }:
let
  cfg = config.process-managers.honcho;
in
{
  options.process-managers.honcho = {
    enable = lib.mkEnableOption "honcho as process-manager";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.honcho;
      defaultText = lib.literalExpression "pkgs.honcho";
      description = "The honcho package to use.";
    };
  };
  config = lib.mkIf cfg.enable {
    processManagerCommand = ''
      ${cfg.package}/bin/honcho start -f ${config.procfile} --env ${config.procfileEnv} "$@" &
    '';

    packages = [ cfg.package ];
  };
}
