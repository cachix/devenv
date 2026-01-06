{ pkgs, config, lib, ... }:

let
  cfg = config.languages.swift;
in
{
  options.languages.swift = {
    enable = lib.mkEnableOption "tools for Swift development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.swift;
      defaultText = lib.literalExpression "pkgs.swift";
      description = ''
        The Swift package to use.
      '';
    };

    lsp = {
      enable = lib.mkEnableOption "Swift Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.sourcekit-lsp;
        defaultText = lib.literalExpression "pkgs.sourcekit-lsp";
        description = "The Swift language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.clang
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;

    env.CC = "${pkgs.clang}/bin/clang";
  };
}
