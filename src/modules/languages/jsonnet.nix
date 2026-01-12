{ pkgs, config, lib, ... }:

let
  cfg = config.languages.jsonnet;
in
{
  options.languages.jsonnet = {
    enable = lib.mkEnableOption "tools for jsonnet development";

    lsp = {
      enable = lib.mkEnableOption "Jsonnet Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.jsonnet-language-server;
        defaultText = lib.literalExpression "pkgs.jsonnet-language-server";
        description = "The Jsonnet language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      pkgs.go-jsonnet
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
