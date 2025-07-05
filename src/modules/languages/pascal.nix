{ pkgs, config, lib, ... }:

let
  cfg = config.languages.pascal;
in
{
  options.languages.pascal = {
    enable = lib.mkEnableOption "tools for Pascal development";

    lazarus = {
      enable = lib.mkEnableOption "lazarus graphical IDE for the FreePascal language";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Pascal development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false; # Disabled by default as no LSP is available in nixpkgs
          description = "Enable Pascal language server.";
        };
        # Pascal LSP servers exist but are not yet available in nixpkgs
        # Available external options include:
        # - pascal-language-server (genericptr/pascal-language-server) - Uses CodeTools from Lazarus
        # - pascal-language-server (arjanadriaanse/pascal-language-server) - Incomplete, only supports code completion
        # - pascal-language-server-isopod (Axiomworks/pascal-language-server-isopod)
        # These require Free Pascal Compiler 3.2.0+ and Lazarus for compilation
        # Once available in nixpkgs, add package option here
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      fpc
    ] ++ lib.optional (cfg.lazarus.enable && pkgs.stdenv.isLinux) pkgs.lazarus;
  };
}
