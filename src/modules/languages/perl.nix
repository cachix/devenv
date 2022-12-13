{ pkgs, config, lib, ... }:

let
  cfg = config.languages.perl;
in
{
  options.languages.perl = {
    enable = lib.mkEnableOption "Enable tools for Perl development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      perl
    ];
  };
}
