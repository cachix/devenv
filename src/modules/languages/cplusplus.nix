{ pkgs, config, lib, ... }:

let
  cfg = config.languages.cplusplus;
  cCfg = config.languages.c;
in
{
  options.languages.cplusplus = {
    enable = lib.mkEnableOption "tools for C++ development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.clang;
      defaultText = lib.literalExpression "pkgs.clang";
      description = "The C++ compiler package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable C++ development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable ccls language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.ccls;
          defaultText = lib.literalExpression "pkgs.ccls";
          description = ''
            The ccls package to use.
            
            Note: clangd (available in pkgs.clang-tools) is the most popular C/C++ LSP,
            actively maintained by the LLVM team. You can switch to it by setting:
            languages.cplusplus.dev.lsp.package = pkgs.clang-tools;
            
            Other LSPs:
            - ccls: Good alternative, currently the default
            - cquery: Deprecated/unmaintained, do not use
          '';
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable clang-format formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.clang-tools;
          defaultText = lib.literalExpression "pkgs.clang-tools";
          description = "The clang-format package to use.";
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
          default =
            if !(pkgs.stdenv.isAarch64 && pkgs.stdenv.isLinux) && lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.gdb
            then pkgs.gdb
            else pkgs.lldb;
          defaultText = lib.literalExpression "pkgs.gdb or pkgs.lldb";
          description = "The debugger package to use. Defaults to gdb if available, otherwise lldb.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      # Core compiler and build tools - always available
      cfg.package
      pkgs.cmake
    ] ++ lib.optionals cfg.dev.enable (
      # Share LSP with C module if both are enabled
      lib.optional (cfg.dev.lsp.enable && !cCfg.enable) cfg.dev.lsp.package ++
        lib.optional (cfg.dev.formatter.enable) cfg.dev.formatter.package ++
        # Share debugger with C module if both are enabled
        lib.optional (cfg.dev.debugger.enable && !cCfg.enable) cfg.dev.debugger.package
    );
  };
}
