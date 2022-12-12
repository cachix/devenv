{ pkgs, config, lib, ... }:

let
  cfg = config.languages.go;
in
{
  options.languages.go = {
    enable = lib.mkEnableOption "Enable tools for Go development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      go
      gotools
    ];
  };
}
