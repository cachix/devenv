{ pkgs, config, lib, ... }:

let
  cfg = config.languages.cue;
in
{
  options.languages.cue = {
    enable = lib.mkEnableOption "tools for Cue development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.cue;
      defaultText = lib.literalExpression "pkgs.cue";
      description = "The CUE package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable CUE development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable cuelsp language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.cuelsp;
          defaultText = lib.literalExpression "pkgs.cuelsp";
          description = "The cuelsp package to use.";
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
