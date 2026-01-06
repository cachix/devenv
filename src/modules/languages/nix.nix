{ pkgs, config, lib, ... }:

let
  cfg = config.languages.nix;
  cachix = lib.getBin config.cachix.package;

  # a bit of indirection to prevent mkShell from overriding the installed Nix
  vulnix = pkgs.buildEnv {
    name = "vulnix";
    paths = [ pkgs.vulnix ];
    pathsToLink = [ "/bin" ];
  };
in
{
  options.languages.nix = {
    enable = lib.mkEnableOption "tools for Nix development";

    lsp = {
      enable = lib.mkEnableOption "Nix Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.nixd;
        defaultText = lib.literalExpression "pkgs.nixd";
        description = "The Nix language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      statix
      deadnix
      vulnix
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package
      ++ lib.optional config.cachix.enable cachix;
  };
}
