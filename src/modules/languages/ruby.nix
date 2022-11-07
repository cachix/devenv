{ pkgs, config, lib, ... }:

let
  cfg = config.languages.ruby;
in
{
  options.languages.ruby = {
    enable = lib.mkEnableOption "Enable tools for Ruby development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      ruby
      bundler
    ];

    enterShell = ''
      ruby --version

      bundler --version
    '';
  };
}
