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

    packageNamespaces = lib.mkOption {
      type = lib.types.attrsOf lib.types.package;
      description = "Packages to add locally, so they can be imported with #import \"@<namespace>/<package>:<version>\".";
      default = { };
      example = lib.literalExpression "{ns1 = fooInput; ns2 = barInput;}";
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

    env.TYPST_FONT_PATHS =
      if cfg.fontPaths != [ ] then (lib.concatStringsSep ":" cfg.fontPaths) else null;

    env.TYPST_PACKAGE_PATH =
      if cfg.packageNamespaces != { } then
        "${pkgs.runCommandLocal "typst-package-path" { } ''
          mkdir $out
          ${lib.concatStringsSep "\n" (
            lib.mapAttrsToList (ns: pkg: ''
              ln -s "${pkg}" "$out/${ns}"
            '') cfg.packageNamespaces
          )}
        ''}"
      else
        null;

  };
}
