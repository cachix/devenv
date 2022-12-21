{ pkgs, config, lib, ... }:

let
  cfg = config.languages.ruby;
in
{
  options.languages.ruby = {
    enable = lib.mkEnableOption "Enable tools for Ruby development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ruby_3_1;
      defaultText = "pkgs.ruby_3_1";
      description = "The Ruby package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
      bundler
    ];

    env.BUNDLE_PATH = config.env.DEVENV_STATE + "/.bundle";

    env.GEM_HOME = "${config.env.BUNDLE_PATH}/${cfg.package.rubyEngine}/${cfg.package.version.libDir}";

    enterShell = ''
      export GEM_PATH="$GEM_HOME/gems:$GEM_PATH"
      export PATH="$GEM_HOME/bin:$PATH"
    '';
  };
}
