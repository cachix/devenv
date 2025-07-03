{ pkgs, config, lib, ... }:

let
  cfg = config.languages.vala;
in
{
  options.languages.vala = {
    enable = lib.mkEnableOption "tools for Vala development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.vala;
      defaultText = lib.literalExpression "pkgs.vala";
      description = "The Vala package to use.";
      example = lib.literalExpression "pkgs.vala_0_54";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Vala development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Vala language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.vala-language-server;
          defaultText = lib.literalExpression "pkgs.vala-language-server";
          description = "The vala-language-server package to use.";
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
