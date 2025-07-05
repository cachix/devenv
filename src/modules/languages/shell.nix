{ pkgs, config, lib, ... }:

let
  cfg = config.languages.shell;
in
{
  options.languages.shell = {
    enable = lib.mkEnableOption "tools for shell development";

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Shell development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable bash language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.nodePackages.bash-language-server;
          defaultText = lib.literalExpression "pkgs.nodePackages.bash-language-server";
          description = "The bash-language-server package to use.";
        };
      };

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable shellcheck linter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.shellcheck;
          defaultText = lib.literalExpression "pkgs.shellcheck";
          description = "The shellcheck package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable shfmt formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.shfmt;
          defaultText = lib.literalExpression "pkgs.shfmt";
          description = "The shfmt package to use.";
        };
      };

      testRunner = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable bats test runner.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.bats.withLibraries (p: [ p.bats-assert p.bats-file p.bats-support ]);
          defaultText = lib.literalExpression "pkgs.bats.withLibraries (p: [ p.bats-assert p.bats-file p.bats-support ])";
          description = "The bats package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
      lib.optional cfg.dev.linter.enable cfg.dev.linter.package ++
      lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package ++
      lib.optional cfg.dev.testRunner.enable cfg.dev.testRunner.package
    );
  };
}
