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

    lsp = {
      enable = lib.mkEnableOption "Typst Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.tinymist;
        defaultText = lib.literalExpression "pkgs.tinymist";
        description = "The Typst language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.typstyle # formatter
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;

    env.TYPST_FONT_PATHS = if cfg.fontPaths != [ ] then (lib.concatStringsSep ":" cfg.fontPaths) else null;
  };
}
