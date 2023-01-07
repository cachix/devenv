{ pkgs, config, lib, ... }:

let
  cfg = config.languages.unison;
in
{
  options.languages.unison = {
    enable = lib.mkEnableOption "Enable tools for Unison development.";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Unison to use";
      default = pkgs.unison-ucm;
      defaultText = lib.literalExpression "pkgs.unison-ucm";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
