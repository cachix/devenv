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
    # enable C tooling by default so native extensions can be built
    languages.c.enable = lib.mkDefault true;

    packages = with pkgs; [
      cfg.package
      bundler
    ];

    env.BUNDLE_PATH = config.env.DEVENV_STATE + "/.bundle";

    env.GEM_HOME = "${config.env.BUNDLE_PATH}/${cfg.package.rubyEngine}/${cfg.package.version.libDir}";

    enterShell =
      let libdir = cfg.package.version.libDir;
      in
      ''
        export RUBYLIB="$DEVENV_PROFILE/${libdir}:$DEVENV_PROFILE/lib/ruby/site_ruby:$DEVENV_PROFILE/lib/ruby/site_ruby/${libdir}:$DEVENV_PROFILE/lib/ruby/site_ruby/${libdir}/${pkgs.system}:$RUBYLIB"
        export GEM_PATH="$GEM_HOME/gems:$GEM_PATH"
        export PATH="$GEM_HOME/bin:$PATH"
      '';
  };
}
