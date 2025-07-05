{ pkgs, config, lib, ... }:

let
  cfg = config.languages.nim;
in
{
  options.languages.nim = {
    enable = lib.mkEnableOption "tools for Nim development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nim;
      defaultText = lib.literalExpression "pkgs.nim";
      description = "The Nim package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Nim development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Nim language server (nimlangserver).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.nimlangserver;
          defaultText = lib.literalExpression "pkgs.nimlangserver";
          description = "The nimlangserver package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable nimpretty formatter.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package
    );
  };
}
