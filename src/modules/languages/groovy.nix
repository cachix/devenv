{ pkgs
, config
, lib
, ...
}:

let
  cfg = config.languages.groovy;
in
{
  options.languages.groovy = {
    enable = lib.mkEnableOption "tools for Groovy development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.groovy;
      defaultText = lib.literalExpression "pkgs.groovy";
      description = "The Groovy package to use.";
    };

    lsp = {
      enable = lib.mkEnableOption "Groovy Language Server" // {
        default = true;
      };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.groovy-language-server;
        defaultText = lib.literalExpression "pkgs.groovy-language-server";
        description = "The Groovy language server package to use.";
      };
    };

    linter = {
      enable = lib.mkEnableOption "Groovy linter, formatter, and auto-fixer. Using npm-groovy-lint" // {
        default = true;
      };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.npm-groovy-lint;
        defaultText = lib.literalExpression "pkgs.npm-groovy-lint";
        description = "The Groovy linter package to use";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ]
    ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
