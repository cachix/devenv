{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.hivemind;
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
  };

  config = lib.mkIf cfg.enable {
    process.manager.args = {
      "print-timestamps" = true;
    };

    process.manager.command = lib.mkDefault ''
      ${cfg.package}/bin/hivemind \
        ${lib.cli.toGNUCommandLineShell {} config.process.manager.args} \
        "$@" ${config.procfile} &
    '';

    packages = [ cfg.package ];
  };
}
