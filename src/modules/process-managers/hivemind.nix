{ pkgs, config, lib, ... }:
let
  cfg = config.process-managers.hivemind;
in
{
  options.process-managers.hivemind = {
    enable = lib.mkEnableOption "hivemind as process-manager";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.hivemind;
      defaultText = lib.literalExpression "pkgs.hivemind";
      description = "The hivemind package to use.";
    };
  };
  config = lib.mkIf cfg.enable {
    processManagerCommand = ''
      ${cfg.package}/bin/hivemind --print-timestamps "$@" ${config.procfile} &
    '';

    packages = [ cfg.package ];
  };
}
