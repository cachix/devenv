{ pkgs, config, lib, ... }:

let
  cfg = config.languages.vala;
in
{
  options.languages.vala = {
    enable = lib.mkEnableOption "tools for Vala development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.vala;
      defaultText = lib.literalExpression "pkgs.vala";
      description = "The Vala package to use.";
      example = lib.literalExpression "pkgs.vala_0_54";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
