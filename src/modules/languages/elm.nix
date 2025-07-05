{ pkgs, config, lib, ... }:

let
  cfg = config.languages.elm;
in
{
  options.languages.elm = {
    enable = lib.mkEnableOption "tools for Elm development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.elmPackages.elm;
      defaultText = lib.literalExpression "pkgs.elmPackages.elm";
      description = "The Elm compiler package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Elm development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable elm-language-server language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.elmPackages.elm-language-server;
          defaultText = lib.literalExpression "pkgs.elmPackages.elm-language-server";
          description = "The elm-language-server package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable elm-format formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.elmPackages.elm-format;
          defaultText = lib.literalExpression "pkgs.elmPackages.elm-format";
          description = "The elm-format package to use.";
        };
      };

      test = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable elm-test test runner.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.elmPackages.elm-test;
          defaultText = lib.literalExpression "pkgs.elmPackages.elm-test";
          description = "The elm-test package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.elm2nix
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package ++
        lib.optional cfg.dev.test.enable cfg.dev.test.package
    );
  };
}
