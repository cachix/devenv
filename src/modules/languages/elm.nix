{ pkgs, config, lib, ... }:

let
  cfg = config.languages.elm;
in
{
  options.languages.elm = {
    enable = lib.mkEnableOption "tools for Elm development";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      elmPackages.elm
      elmPackages.elm-format
      elmPackages.elm-test
      elmPackages.elm-language-server
      elm2nix
    ];
  };
}
