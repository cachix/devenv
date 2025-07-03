{ pkgs, config, lib, ... }:

let
  cfg = config.languages.odin;
in
{
  options.languages.odin = {
    enable = lib.mkEnableOption "tools for Odin Language";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.odin;
      defaultText = lib.literalExpression "pkgs.odin";
      description = "The odin package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Odin development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Odin language server (ols).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.ols;
          defaultText = lib.literalExpression "pkgs.ols";
          description = "The ols package to use.";
        };
      };

      debugger = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable gdb debugger.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.gdb;
          defaultText = lib.literalExpression "pkgs.gdb";
          description = "The gdb package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      nasm
      clang
      gnumake
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional (cfg.dev.lsp.enable && lib.meta.availableOn pkgs.stdenv.hostPlatform cfg.dev.lsp.package) cfg.dev.lsp.package ++
        lib.optional (cfg.dev.debugger.enable && lib.meta.availableOn pkgs.stdenv.hostPlatform cfg.dev.debugger.package) cfg.dev.debugger.package
    );
  };
}
