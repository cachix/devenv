{ pkgs, config, lib, ... }:

let cfg = config.languages.idris;
in {
  options.languages.idris = {
    enable = lib.mkEnableOption "tools for Idris development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.idris2;
      defaultText = lib.literalExpression "pkgs.idris2";
      description = ''
        The Idris package to use.
      '';
      example = lib.literalExpression "pkgs.idris";
    };

    lsp = {
      enable = lib.mkEnableOption "Idris Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.idris2Packages.idris2Lsp;
        defaultText = lib.literalExpression "pkgs.idris2Packages.idris2Lsp";
        description = "The Idris language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
