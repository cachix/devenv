{ pkgs, config, lib, ... }:

let
  cfg = config.languages.kotlin;
in
{
  options.languages.kotlin = {
    enable = lib.mkEnableOption "tools for Kotlin development";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      kotlin
      gradle
    ];
  };
}
