{ pkgs, config, lib, ... }:

let
  cfg = config.languages.crystal;
in
{
  options.languages.crystal = {
    enable = lib.mkEnableOption "Enable tools for Crystal development.";
  };

  config = lib.mkIf cfg.enable {
    # enable compiler tooling by default to expose things like cc
    languages.c.enable = lib.mkDefault true;

    packages = [
      pkgs.crystal
      pkgs.shards
    ];
  };
}
