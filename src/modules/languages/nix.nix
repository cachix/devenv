{ pkgs, config, lib, ... }:

let
  cfg = config.languages.nix;
in
{
  options.languages.nix = {
    enable = lib.mkEnableOption "Enable tools for Nix development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cachix
      statix
      vulnix
      deadnix
      nil
    ];

    enterShell = ''
      deadnix --version
      cachix --version
      statix --version
      vulnix --version
      nil --version
    '';
  };
}
