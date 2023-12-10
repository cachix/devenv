{ pkgs, config, lib, ... }:

let
  cfg = config.languages.purescript;
  # supported via rosetta
  supportAarch64Darwin = package: package.overrideAttrs (attrs: {
    meta = attrs.meta // {
      platforms = lib.platforms.linux ++ lib.platforms.darwin;
    };
  });
in
{
  options.languages.purescript = {
    enable = lib.mkEnableOption "tools for PureScript development";

    package = lib.mkOption {
      type = lib.types.package;
      default = (supportAarch64Darwin pkgs.purescript);
      defaultText = lib.literalExpression "pkgs.purescript";
      description = "The PureScript package to use.";
    };

  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.nodePackages.purescript-language-server
      pkgs.nodePackages.purs-tidy
      pkgs.purescript-psa
      (supportAarch64Darwin pkgs.psc-package)
    ];
  };
}
