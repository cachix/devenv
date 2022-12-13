{ pkgs, config, lib, ... }:

let
  cfg = config.languages.haskell;
in
{
  options.languages.haskell = {
    enable = lib.mkEnableOption "Enable tools for Haskell development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      stack
      cabal-install
      zlib
      hpack
    ];
  };
}
