{ pkgs
, lib
, config
, ...
}:

let
  cfg = config.gitnr;

  ignoreFileSubmodule =
    { ... }:
    {
      options = {
        enable = lib.mkEnableOption "Enable gitnr integration.";

        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.gitnr;
          defaultText = lib.literalExpression "pkgs.gitnr";
          description = "The gitnr package to use for generating templates.";
        };

        content = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          example = [
            "*.log"
            "dist/"
          ];
          description = ''
            Additional patterns to append to the generated ignore file.
            These patterns will be added after the templates are processed.
          '';
        };

        enableDefaultTemplates = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Prepend a sensible default set of GitHub Global templates.";
        };

        templates = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
          example = [
            "tt:linux"
            "tt:macos"
            "tt:windows"
          ];
          description = ''
            List of templates to include in the ignore file.

            Template strings are passed directly to `gitnr create`.
          '';
        };
      };
    };

  defaultTemplates =
    let
      os = [
        "ghg:linux"
        "ghg:macos"
        "ghg:windows"
      ];
      editors = [
        "ghg:jetbrains"
        "ghg:vim"
        "ghg:visualstudiocode"
        "ghg:sublimetext"
        "ghg:emacs"
        "ghg:eclipse"
      ];
      vcs = [
        "ghg:svn"
        "ghg:mercurial"
        "ghg:tortoisegit"
      ];

    in
    os ++ editors ++ vcs;

  mkTemplates =
    fileCfg: fileCfg.templates ++ lib.optionals fileCfg.enableDefaultTemplates defaultTemplates;

  mkContent =
    contentLines:
    if contentLines == [ ] then "" else lib.concatStringsSep "\n" (contentLines ++ [ "" ]);

  mkFileExec =
    filename: fileCfg:
    let
      templates = mkTemplates fileCfg;
      content = mkContent fileCfg.content;
      outPath = "${config.env.DEVENV_ROOT}/${filename}";

      gitnrArgs = templates ++ lib.optional (fileCfg.content != [ ]) "file:/dev/stdin";

      gitnrCmd =
        if fileCfg.content == [ ] then
          "${lib.getExe fileCfg.package} create ${lib.concatStringsSep " " gitnrArgs}"
        else
          "${lib.getExe' pkgs.coreutils "printf"} '%s' ${lib.escapeShellArg content} | ${lib.getExe fileCfg.package} create ${lib.concatStringsSep " " gitnrArgs}";

      shouldGenerate = templates != [ ] || fileCfg.content != [ ];
    in
    lib.optionalString shouldGenerate ''
      ${gitnrCmd} | ${lib.getExe' pkgs.moreutils "sponge"} ${lib.escapeShellArg outPath}
    '';

  fileExecs = lib.filter (s: s != "") (lib.mapAttrsToList mkFileExec cfg);
in
{
  options.gitnr = lib.mkOption {
    type = lib.types.attrsOf (lib.types.submodule ignoreFileSubmodule);
    default = { };
    example = lib.literalExpression ''
      {
        ".gitignore" = {
          enableDefaultTemplates = true;
          templates = [ "tt:go" "tt:node" ];
          content = [
            "*.env"
          ];
        };
      }
    '';
    description = "Declarative generation of ignore files using gitnr templates.";
  };

  config = lib.mkIf (cfg != { }) {
    tasks."devenv:gitnr:install" = {
      before = [ "devenv:enterShell" ];
      description = "Generate ignore files";
      exec = lib.concatStringsSep "\n" fileExecs;
    };
  };
}
