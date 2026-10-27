{ pkgs, lib, config, ... }:

let
  cfg = config.changelog-merge;
  driver = "${lib.escapeShellArg (lib.getExe cfg.package)} merge %O %A %B --marker-size %L --path %P";
in
{
  options.changelog-merge = {
    enable = lib.mkEnableOption "semantic Git merging for Keep a Changelog files";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.changelog-merge;
      defaultText = lib.literalExpression "pkgs.changelog-merge";
      description = "The changelog-merge package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];

    git.config.merge.changelog-merge = {
      name = "Keep a Changelog semantic merger";
      inherit driver;
      recursive = "binary";
    };

    git.attributes = [ "CHANGELOG.md merge=changelog-merge" ];
  };
}
