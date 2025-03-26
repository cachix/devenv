{ pkgs, config, lib, ... }:

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
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.tinymist # lsp
      pkgs.typstyle # formatter
    ];

    env.TYPST_FONT_PATHS = lib.concatStringsSep ":" cfg.fontPaths;
  };
}
