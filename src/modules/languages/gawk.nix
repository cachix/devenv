{ pkgs, config, lib, ... }:

let
  cfg = config.languages.gawk;
in
{
  options.languages.gawk = {
    enable = lib.mkEnableOption "tools for GNU Awk development";

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable GNU Awk development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable awk-language-server language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.awk-language-server;
          defaultText = lib.literalExpression "pkgs.awk-language-server";
          description = "The awk-language-server package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      gawk
      gawkextlib.gawkextlib
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package
    );
  };
}
