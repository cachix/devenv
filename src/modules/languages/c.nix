{ pkgs, config, lib, ... }:

let
  cfg = config.languages.c;
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "languages" "c" "debugger" ] [ "languages" "c" "dev" "debugger" "package" ])
  ];

  options.languages.c = {
    enable = lib.mkEnableOption "tools for C development";

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable C development tools.";
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
            languages.c.dev.lsp.package = pkgs.clang-tools;
            
            Other LSPs:
            - ccls: Good alternative, currently the default
            - cquery: Deprecated/unmaintained, do not use
          '';
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

      valgrind = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.valgrind && !pkgs.valgrind.meta.broken;
          defaultText = lib.literalExpression "lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.valgrind && !pkgs.valgrind.meta.broken";
          description = "Enable valgrind memory debugger.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.valgrind;
          defaultText = lib.literalExpression "pkgs.valgrind";
          description = "The valgrind package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      pkgs.stdenv
      pkgs.gnumake
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional (cfg.dev.lsp.enable) cfg.dev.lsp.package ++
        lib.optional (cfg.dev.debugger.enable && !(pkgs.stdenv.isAarch64 && pkgs.stdenv.isLinux) && lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.gdb) cfg.dev.debugger.package ++
        lib.optional (cfg.dev.valgrind.enable) cfg.dev.valgrind.package
    );
  };
}
