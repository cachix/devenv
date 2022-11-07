{ pkgs, config, lib, ... }:

let
  cfg = config.languages.rust;
in
{
  options.languages.rust = {
    enable = lib.mkEnableOption "Enable tools for Rust development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      rustc
      cargo
    ];

    enterShell = ''
      rustc --version
      cargo --version
    '';
  };
}
