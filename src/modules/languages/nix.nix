{ pkgs, config, lib, ... }:

let
  cfg = config.languages.nix;
in
{
  options.languages.nix = {
    enable = lib.mkEnableOption "tools for Nix development";
    lsp.package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nil;
      defaultText = lib.literalExpression "pkgs.nil";
      description = "The LSP package to use";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cachix
      statix
      vulnix
      deadnix
      cfg.lsp.package
    ];
  };
}
