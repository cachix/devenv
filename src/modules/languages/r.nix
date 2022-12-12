{ pkgs, config, lib, ... }:

let
  cfg = config.languages.r;
in
{
  options.languages.r = {
    enable = lib.mkEnableOption "Enable tools for R development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      R
    ];
  };
}
