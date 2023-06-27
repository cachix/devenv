{ pkgs, config, lib, ... }:

let
  cfg = config.languages.jsonnet;
in
{
  options.languages.jsonnet = {
    enable = lib.mkEnableOption "tools for jsonnet development";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      go-jsonnet
    ];
  };
}
