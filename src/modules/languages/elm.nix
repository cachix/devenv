{ pkgs, config, lib, ... }:

let
  cfg = config.languages.elm;
in
{
  options.languages.elm = {
    enable = lib.mkEnableOption "Enable tools for Elm development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      elmPackages.elm
      elmPackages.elm-format
      elmPackages.elm-language-server
      elm2nix
    ];

    enterShell = ''
      echo elm --version
      elm --version

      which elm-format

      echo elm2nix --version
      which elm2nix
    '';
  };
}
