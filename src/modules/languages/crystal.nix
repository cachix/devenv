{ pkgs, config, lib, ... }:

let
  cfg = config.languages.crystal;
in
{
  options.languages.crystal = {
    enable = lib.mkEnableOption "Enable tools for Crystal development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.crystal;
      defaultText = lib.literalExpression "pkgs.crystal";
      description = "The Crystal package to use.";
    };

    shards_package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.shards;
      defaultText = lib.literalExpression "pkgs.shards";
      description = "The Shards package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    # enable compiler tooling by default to expose things like cc
    languages.c.enable = lib.mkDefault true;

    packages = [
      cfg.package
      cfg.shards_package
    ];
  };
}
