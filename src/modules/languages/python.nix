{ pkgs, config, lib, ... }:

let
  cfg = config.languages.python;
in
{
  options.languages.python = {
    enable = lib.mkEnableOption "Enable tools for Python development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      python3
    ];

    enterShell = ''
      python --version
    '';
  };
}
