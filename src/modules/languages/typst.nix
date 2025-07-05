{ pkgs
, config
, lib
, ...
}:

let
  cfg = config.languages.typst;
in
{
  options.languages.typst = {
    enable = lib.mkEnableOption "tools for Typst development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Typst to use.";
      default = pkgs.typst;
      defaultText = lib.literalExpression "pkgs.typst";
    };

    fontPaths = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      description = "Directories to be searched for fonts.";
      default = [ ];
      defaultText = lib.literalExpression "[]";
      example = lib.literalExpression ''[ "''${pkgs.roboto}/share/fonts/truetype" ]'';
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Typst development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Typst language server (tinymist).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.tinymist;
          defaultText = lib.literalExpression "pkgs.tinymist";
          description = "The tinymist package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Typst formatter (typstyle).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.typstyle;
          defaultText = lib.literalExpression "pkgs.typstyle";
          description = "The typstyle package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package
    );

    env.TYPST_FONT_PATHS = if cfg.fontPaths != [ ] then (lib.concatStringsSep ":" cfg.fontPaths) else null;
  };
}
