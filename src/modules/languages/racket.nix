{ pkgs, config, lib, ... }:

let
  cfg = config.languages.racket;
in
{
  options.languages.racket = {
    enable = lib.mkEnableOption "tools for Racket development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.racket-minimal;
      defaultText = lib.literalExpression "pkgs.racket-minimal";
      description = "The Racket package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Racket development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false; # Disabled by default as no LSP is available in nixpkgs
          description = "Enable Racket language server.";
        };
        # racket-langserver exists but is not yet available in nixpkgs
        # There is an open package request: https://github.com/NixOS/nixpkgs/issues/333113
        # Available external options include:
        # - racket-langserver (jeapostrophe/racket-langserver) - Uses DrRacket's APIs
        # - racket-language-server (theia-ide/racket-language-server) - Alternative implementation
        # Can be installed via Racket's package system: raco pkg install racket-langserver
        # Once available in nixpkgs, add package option here
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
