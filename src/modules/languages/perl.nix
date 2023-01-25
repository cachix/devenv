{ pkgs, config, lib, ... }:

let
  cfg = config.languages.perl;
in
{
  options.languages.perl = {
    enable = lib.mkEnableOption "tools for Perl development";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      perl
    ];
  };
}
