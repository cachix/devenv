{ pkgs, config, lib, ... }:

let
  cfg = config.languages.javascript;
in
{
  options.languages.javascript = {
    enable = lib.mkEnableOption "Enable tools for JavaScript development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nodejs;
      defaultText = lib.literalExpression "pkgs.nodejs";
      description = "The Node package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
