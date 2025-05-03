{ pkgs, config, lib, ... }:

let
  cfg = config.languages.nim;
in
{
  options.languages.nim = {
    enable = lib.mkEnableOption "tools for Nim development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nim;
      defaultText = lib.literalExpression "pkgs.nim";
      description = "The Nim package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.nimlangserver
    ];
  };
}
