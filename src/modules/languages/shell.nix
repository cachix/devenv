{ pkgs, config, lib, ... }:

let
  cfg = config.languages.shell;
in
{
  options.languages.shell = {
    enable = lib.mkEnableOption "tools for shell development";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      bats
      nodePackages.bash-language-server
      shellcheck
      shfmt
    ];
  };
}
