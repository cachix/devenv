{
  pkgs,
  config,
  lib,
  ...
}:

let
  cfg = config.languages.lobster;
in
{
  options.languages.lobster = {
    enable = lib.mkEnableOption "tools for Lobster development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which Lobster package to use.";
      default = pkgs.lobster;
      defaultText = lib.literalExpression "pkgs.lobster";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
