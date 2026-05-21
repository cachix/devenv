{ pkgs, config, lib, ... }:

let
  cfg = config.languages.cplusplus;
in
{
  options.languages.cplusplus = {
    enable = lib.mkEnableOption "tools for C++ development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.clang;
      defaultText = lib.literalExpression "pkgs.clang";
      description = "The C++ compiler to use.";
    };

    cmake = lib.mkOption {
      type = lib.types.submodule {
        options.package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.cmake;
          defaultText = lib.literalExpression "pkgs.cmake";
          description = "The CMake package to use.";
        };
      };
      description = "Configuration for cmake";
      default = { };
    };

    tools = {
      enable = lib.mkEnableOption "Standalone command line tools for C++ development" // {
        default = cfg.package.isClang;
        defaultText = lib.literalMD "Enabled by default for clang-based compilers";
      };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.clang-tools;
        defaultText = lib.literalExpression "pkgs.clang-tools";
        description = "The C++ command line tools package to use.";
      };
    };

    lsp = {
      enable = lib.mkEnableOption "C++ Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.ccls;
        defaultText = lib.literalExpression "pkgs.ccls";
        description = "The C++ language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.cmake.package
      cfg.package
    ]
    ++ lib.optional cfg.tools.enable cfg.tools.package
    ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
