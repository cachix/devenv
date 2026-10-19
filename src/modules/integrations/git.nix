{ pkgs, config, lib, ... }:

let
  cfg = config.git;
  gitFormat = pkgs.formats.gitIni { listsAsDuplicateKeys = true; };
  attributesFile = pkgs.writeText "devenv-gitattributes" (lib.concatLines cfg.attributes);
  configFile = gitFormat.generate "devenv-gitconfig" cfg.config;
in
{
  options.git = {
    root = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      description = "Git repository root path. This field is populated automatically in devenv 1.10 and newer.";
      default = null;
    };

    config = lib.mkOption {
      type = gitFormat.type;
      default = { };
      example = {
        merge.conflictStyle = "diff3";
        merge.tool = "vimdiff";
        diff.algorithm = "histogram";
      };
      description = ''
        Git configuration active in the devenv environment. Sections and
        subsections are represented by nested attribute sets.

        These settings override configuration files, including repository-local
        settings, but can be overridden with `git -c`. Existing user and system
        configuration is still read. This uses `GIT_CONFIG_COUNT`,
        `GIT_CONFIG_KEY_0`, and `GIT_CONFIG_VALUE_0`; do not set those separately.
        The generated file is stored in the Nix store and must not contain secrets.
      '';
    };

    attributes = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "*.nix diff=nix" "*.png binary" ];
      description = ''
        Git attribute rules active in the devenv environment. When non-empty,
        these replace the user's global attributes file via `core.attributesFile`.
        Repository `.gitattributes` and `.git/info/attributes` still take precedence.
        No files are written to the repository.
      '';
    };
  };

  config = lib.mkMerge [
    (lib.mkIf (cfg.attributes != [ ]) {
      git.config.core.attributesFile = lib.mkDefault (toString attributesFile);
    })
    (lib.mkIf (cfg.config != { }) {
      env = {
        GIT_CONFIG_COUNT = "1";
        GIT_CONFIG_KEY_0 = "include.path";
        GIT_CONFIG_VALUE_0 = toString configFile;
      };
    })
  ];
}
