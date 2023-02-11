{ pkgs, config, lib, ... }:

let
  cfg = config.languages.crystal;
in
{
  options.languages.crystal = {
    enable = lib.mkEnableOption "Enable tools for Crystal development.";
  };

  config = lib.mkIf cfg.enable {
    packages = [
      pkgs.crystal
      pkgs.shards
    ];
  };
}
