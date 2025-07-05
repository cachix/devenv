{ pkgs, config, lib, ... }:

let
  cfg = config.languages.sql;
in
{
  options.languages.sql = {
    enable = lib.mkEnableOption "tools for SQL development";

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable SQL development tools.";
      };

      # SQL Language Server Protocol (LSP) options
      # sqls is a SQL language server that provides completion, hover, and diagnostics
      # for various SQL dialects including PostgreSQL, MySQL, SQLite, and more
      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable sqls language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.sqls;
          defaultText = lib.literalExpression "pkgs.sqls";
          description = "The sqls package to use.";
        };
      };

      # SQL formatters
      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable sqlfluff formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.sqlfluff;
          defaultText = lib.literalExpression "pkgs.sqlfluff";
          description = "The sqlfluff package to use.";
        };
      };

      # SQL linter
      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable sqlfluff linter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.sqlfluff;
          defaultText = lib.literalExpression "pkgs.sqlfluff";
          description = "The sqlfluff package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages =
      # Development tools
      lib.optionals cfg.dev.enable (
        lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package ++
        lib.optional cfg.dev.linter.enable cfg.dev.linter.package
      );
  };
}
