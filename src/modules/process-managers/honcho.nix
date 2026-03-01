{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.honcho;
in
{
  options.process.managers.honcho = {
    enable = lib.mkEnableOption "honcho as the process manager" // {
      internal = true;
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.honcho;
      defaultText = lib.literalExpression "pkgs.honcho";
      description = "The honcho package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    process.manager.args = {
      "f" = config.procfile;
    };

    process.manager.command = lib.mkDefault ''
      ${lib.getExe cfg.package} start \
        ${(lib.cli.toCommandLineShellGNU or lib.cli.toGNUCommandLineShell) {} config.process.manager.args} \
        "$@" &
    '';

    packages = [ cfg.package ];
  };
}
