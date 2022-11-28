{ pkgs, config, lib, ... }:

let
  cfg = config.languages.ruby;
in
{
  options.languages.ruby = {
    enable = lib.mkEnableOption "Enable tools for Ruby development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ruby;
      defaultText = "pkgs.ruby";
      description = "The Ruby package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
      bundler
    ];

    env.BUNDLE_PATH = config.env.DEVENV_STATE + "/.bundle";

    enterShell = ''
      ruby --version

      bundler --version
    '';
  };
}
