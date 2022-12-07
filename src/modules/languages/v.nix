{ pkgs, config, lib, ... }:

let
  cfg = config.languages.v;
in
{
  options.languages.v = {
    enable = lib.mkEnableOption "Enable tools for v development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.vlang;
      defaultText = "pkgs.vlang";
      description = "The v package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    enterShell = ''
      v --version
    '';
  };
}
