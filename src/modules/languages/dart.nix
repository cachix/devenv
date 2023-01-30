{ pkgs, config, lib, ... }:

let
  cfg = config.languages.dart;
in
{
  options.languages.dart = {
    enable = lib.mkEnableOption "tools for Dart development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.dart;
      defaultText = lib.literalExpression "pkgs.dart";
      description = "The Dart package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
