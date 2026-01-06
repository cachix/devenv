{ pkgs
, config
, lib
, ...
}:

let
  cfg = config.languages.purescript;

  purescript-overlay = config.lib.getInput {
    name = "purescript-overlay";
    url = "github:thomashoneyman/purescript-overlay";
    attribute = "languages.purescript.enable";
    follows = [ "nixpkgs" ];
  };

  # supported via rosetta
  supportAarch64Darwin = package: package.overrideAttrs (attrs: {
    meta = attrs.meta // {
      platforms = lib.platforms.linux ++ lib.platforms.darwin;
    };
  });
in
{
  options.languages.purescript = {
    enable = lib.mkEnableOption "tools for PureScript development";

    package = lib.mkOption {
      type = lib.types.package;
      default = purescript-overlay.packages.${pkgs.stdenv.system}.purs;
      defaultText = lib.literalExpression "purescript-overlay.packages.\${pkgs.stdenv.system}.purs";
      description = ''
        The PureScript compiler package to use.
        Uses [purescript-overlay](https://github.com/thomashoneyman/purescript-overlay) by default.
      '';
    };

    spago = {
      enable = lib.mkEnableOption "Spago package manager" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = purescript-overlay.packages.${pkgs.stdenv.system}.spago;
        defaultText = lib.literalExpression "purescript-overlay.packages.\${pkgs.stdenv.system}.spago";
        description = ''
          The Spago package manager to use.
          Uses [purescript-overlay](https://github.com/thomashoneyman/purescript-overlay) by default.
        '';
      };
    };

    lsp = {
      enable = lib.mkEnableOption "PureScript Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = purescript-overlay.packages.${pkgs.stdenv.system}.purescript-language-server;
        defaultText = lib.literalExpression "purescript-overlay.packages.\${pkgs.stdenv.system}.purescript-language-server";
        description = ''
          The PureScript language server package to use.
          Uses [purescript-overlay](https://github.com/thomashoneyman/purescript-overlay) by default.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ]
    ++ lib.optional cfg.spago.enable cfg.spago.package
    ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
