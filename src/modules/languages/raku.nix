{ pkgs, config, lib, ... }:

let
  cfg = config.languages.raku;
in
{
  options.languages.perl = {
    enable = lib.mkEnableOption "Enable tools for Raku development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      rakudo
    ];
  };
}
