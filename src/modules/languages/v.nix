{ pkgs, config, lib, ... }:

let
  cfg = config.languages.v;
in
{
  options.languages.v = {
    enable = lib.mkEnableOption "tools for V development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.vlang;
      defaultText = lib.literalExpression "pkgs.vlang";
      description = "The V package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
