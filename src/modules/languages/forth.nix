{ pkgs, config, lib, ... }:

let
  cfg = config.languages.forth;
in
{
  options.languages.forth = {
    enable = lib.mkEnableOption "tools for Forth development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.gforth;
      defaultText = lib.literalExpression "pkgs.gforth";
      description =
	''
	  The Forth package to use, defaults to GNU projects forth implementation.
	  Other Forth implementations available in nixpkgs: pkgs._4th and pkgs.pforth.
	'';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
