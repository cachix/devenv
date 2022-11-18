{ pkgs, config, lib, ... }:

let
  cfg = config.languages.python;
in
{
  options.languages.python = {
    enable = lib.mkEnableOption "Enable tools for Python development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.python3;
      defaultText = "pkgs.python3";
      description = "The Python package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
    ];

    enterShell = ''
      python --version
    '';
  };
}
