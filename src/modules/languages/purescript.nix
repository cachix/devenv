{ pkgs, config, lib, ... }:

let
  cfg = config.languages.purescript;
in
{
  options.languages.purescript = {
    enable = lib.mkEnableOption "Enable tools for PureScript development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.purescript;
      defaultText = "pkgs.purescript";
      description = "The PureScript package to use.";
    };

  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.nodePackages.purescript-language-server
      pkgs.nodePackages.purs-tidy
      pkgs.spago
      pkgs.purescript-psa
      pkgs.psc-package
    ];

    enterShell = ''
      purs --version
    '';
  };
}
