{ pkgs, config, lib, ... }:

let
  cfg = config.languages.shell;
in
{
  options.languages.shell = {
    enable = lib.mkEnableOption "tools for shell development";

    lsp = {
      enable = lib.mkEnableOption "Shell Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.bash-language-server;
        defaultText = lib.literalExpression "pkgs.bash-language-server";
        description = "The Shell language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      (pkgs.bats.withLibraries (p: [ p.bats-assert p.bats-file p.bats-support ]))
      shellcheck
      shfmt
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
