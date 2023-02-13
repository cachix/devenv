{ pkgs, config, lib, inputs, ... }:

let
  cfg = config.languages.ruby;

  nixpkgs-ruby = inputs.nixpkgs-ruby or (throw ''
    To use languages.ruby.version or languages.ruby.versionFile, you need to add the following to your devenv.yaml:
    
      inputs:
        nixpkgs-ruby:
          url: github:bobvanderlinden/nixpkgs-ruby
  '');
in
{
  options.languages.ruby = {
    enable = lib.mkEnableOption "tools for Ruby development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ruby_3_1;
      defaultText = lib.literalExpression "pkgs.ruby_3_1";
      description = "The Ruby package to use.";
    };

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        The Ruby version to use.
        This automatically sets the `languages.ruby.package` using [nixpkgs-ruby](https://github.com/bobvanderlinden/nixpkgs-ruby).
      '';
      example = "3.2.1";
    };

    versionFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        The .ruby-version file path to extract the Ruby version from.
        This automatically sets the `languages.ruby.package` using [nixpkgs-ruby](https://github.com/bobvanderlinden/nixpkgs-ruby).
        When the `.ruby-version` file exists in the same directory as the devenv configuration, you can use:
        
        ```nix
        languages.ruby.versionFile = ./.ruby-version;
        ```
      '';
      example = lib.literalExpression ''
        ./ruby-version
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    # enable C tooling by default so native extensions can be built
    languages.c.enable = lib.mkDefault true;

    languages.ruby.package =
      let
        packageFromVersion = lib.mkIf (cfg.version != null) (
          nixpkgs-ruby.packages.${pkgs.system}."ruby-${cfg.version}"
        );
        packageFromVersionFile = lib.mkIf (cfg.versionFile != null) (
          nixpkgs-ruby.lib.packageFromRubyVersionFile {
            file = cfg.versionFile;
            system = pkgs.system;
          }
        );
      in
      lib.mkMerge [
        packageFromVersion
        packageFromVersionFile
      ];

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
