{ pkgs, config, lib, ... }:

let
  cfg = config.languages.raku;
in
{
  options.languages.raku = {
    enable = lib.mkEnableOption "tools for Raku development";

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Raku development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false; # Disabled by default as no LSP is available in nixpkgs
          description = "Enable Raku language server.";
        };
        # Raku LSP implementations exist but are not yet available in nixpkgs
        # Available external options include:
        # - RakuNavigator (bscan/RakuNavigator) - Most mature, designed for VS Code but can work with other editors
        # - raku-lsp (arunvickram/raku-lsp) - Alternative implementation
        # RakuNavigator requires Node.js and manual installation
        # Community continues to work on better LSP implementations
        # Once available in nixpkgs, add package option here
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      rakudo
    ];
  };
}
