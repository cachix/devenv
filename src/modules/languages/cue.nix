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

    lsp = {
      enable = lib.mkEnableOption "CUE Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.cuelsp;
        defaultText = lib.literalExpression "pkgs.cuelsp";
        description = "The CUE language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
