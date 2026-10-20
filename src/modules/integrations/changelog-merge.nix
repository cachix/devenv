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

    # Append to Git's runtime configuration so existing entries are preserved.
    # Track `CHANGELOG.md merge=changelog-merge` in .gitattributes to opt in.
    enterShell = ''
      _devenv_changelog_merge_count=''${GIT_CONFIG_COUNT:-0}
      export "GIT_CONFIG_KEY_$_devenv_changelog_merge_count=merge.changelog-merge.name"
      export "GIT_CONFIG_VALUE_$_devenv_changelog_merge_count=Keep a Changelog semantic merger"
      export "GIT_CONFIG_KEY_$((_devenv_changelog_merge_count + 1))=merge.changelog-merge.driver"
      export "GIT_CONFIG_VALUE_$((_devenv_changelog_merge_count + 1))="${lib.escapeShellArg driver}
      export "GIT_CONFIG_KEY_$((_devenv_changelog_merge_count + 2))=merge.changelog-merge.recursive"
      export "GIT_CONFIG_VALUE_$((_devenv_changelog_merge_count + 2))=binary"
      export GIT_CONFIG_COUNT=$((_devenv_changelog_merge_count + 3))
      unset _devenv_changelog_merge_count
    '';
  };
}
