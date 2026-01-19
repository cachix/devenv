{ pkgs, config, lib, ... }:

let
  cfg = config.languages.hare;
in
{
  options.languages.hare = {
    enable = lib.mkEnableOption "tools for Hare development";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      hare
      haredoc
    ];
  };
}
