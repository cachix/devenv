{ pkgs, config, lib, ... }:

let
  cfg = config.languages.elm;
in
{
  options.languages.elm = {
    enable = lib.mkEnableOption "tools for Elm development";

    lsp = {
      enable = lib.mkEnableOption "Elm Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.elmPackages.elm-language-server;
        defaultText = lib.literalExpression "pkgs.elmPackages.elm-language-server";
        description = "The Elm language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      elmPackages.elm
      elmPackages.elm-format
      elmPackages.elm-test
      elm2nix
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
