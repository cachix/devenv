{ pkgs, config, lib, ... }:

let
  cfg = config.languages.deno;
in
{
  options.languages.deno = {
    enable = lib.mkEnableOption "Enable tools for Deno development.";
  };

  config = lib.mkIf cfg.enable {
    packages = [
      pkgs.deno
    ];
  };
}
