{ pkgs, config, lib, ... }:

let
  cfg = config.languages.racket;
in
{
  options.languages.racket = {
    enable = lib.mkEnableOption "tools for Racket development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.racket-minimal;
      defaultText = lib.literalExpression "pkgs.racket-minimal";
      description = "The Racket package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
