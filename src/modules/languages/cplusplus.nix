{ pkgs, config, lib, ... }:

let
  cfg = config.languages.cplusplus;
in
{
  options.languages.cplusplus = {
    enable = lib.mkEnableOption "tools for C++ development";

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
      clang-tools
      cmake
      clang
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
