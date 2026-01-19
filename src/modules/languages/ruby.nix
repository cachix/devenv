{ pkgs
, config
, lib
, ...
}:

let
  cfg = config.languages.ruby;

  # Whether using a custom Ruby version (from nixpkgs-ruby)
  usingCustomRuby = cfg.version != null || cfg.versionFile != null;

  # Wrap a Ruby gem package to only expose binaries, without propagating
  # gem dependencies to the environment. The gem's own wrapper script
  # handles its dependencies, so propagation just causes conflicts.
  wrapGemBin = pkg: pkgs.runCommand "${pkg.name}-bin" { } ''
    mkdir -p $out/bin
    for f in ${pkg}/bin/*; do
      ln -s "$f" "$out/bin/$(basename "$f")"
    done
  '';

  # Build solargraph LSP with the user's Ruby using bundlerEnv.
  # This ensures native extensions are compiled against the correct Ruby.
  lspEnv = pkgs.bundlerEnv {
    name = "solargraph-lsp";
    ruby = cfg.package;
    gemdir = ../lib/ruby-lsp;
  };

  lspPackage =
    if usingCustomRuby
    # Use bundlerEnv to build solargraph with the user's Ruby,
    # ensuring native extensions are compiled against the correct Ruby.
    then lspEnv
    # Use pre-built solargraph, wrapped to prevent its gem dependencies
    # from polluting the user's GEM_PATH (avoiding version conflicts).
    else wrapGemBin cfg.lsp.package;

  nixpkgs-ruby = config.lib.getInput {
    name = "nixpkgs-ruby";
    url = "github:bobvanderlinden/nixpkgs-ruby";
    attribute = "languages.ruby.version or languages.ruby.versionFile";
    follows = [ "nixpkgs" ];
  };
in
{
  options.languages.ruby = {
    enable = lib.mkEnableOption "tools for Ruby development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ruby;
      defaultText = lib.literalExpression "pkgs.ruby";
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
        ./.ruby-version
      '';
    };

    bundler = {
      enable = lib.mkEnableOption "bundler";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.bundler.override { ruby = cfg.package; };
        defaultText = lib.literalExpression "pkgs.bundler.override { ruby = cfg.package; }";
        description = "The bundler package to use.";
      };
    };

    documentation = {
      enable = lib.mkEnableOption "documentation support for Ruby packages";
    };

    lsp = {
      enable = lib.mkEnableOption "Ruby Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.solargraph.override { ruby = cfg.package; };
        defaultText = lib.literalExpression "pkgs.solargraph.override { ruby = cfg.package; }";
        description = "The Ruby language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.version == null || cfg.versionFile == null;
        message = ''
          `languages.ruby.version` and `languages.ruby.versionFile` are both set.
          Only one of the two may be set. Remove one of the two options.
        '';
      }
    ];

    # enable C tooling by default so native extensions can be built
    languages.c.enable = lib.mkDefault true;

    languages.ruby.bundler.enable = lib.mkDefault true;

    languages.ruby.package =
      let
        packageFromVersion = lib.mkIf (cfg.version != null) (
          nixpkgs-ruby.packages.${pkgs.stdenv.system}."ruby-${cfg.version}".override {
            docSupport = cfg.documentation.enable;
          }
        );
        packageFromVersionFile = lib.mkIf (cfg.versionFile != null) (
          (nixpkgs-ruby.lib.packageFromRubyVersionFile {
            file = cfg.versionFile;
            system = pkgs.stdenv.system;
          }).override
            {
              docSupport = cfg.documentation.enable;
            }
        );
      in
      lib.mkMerge [
        packageFromVersion
        packageFromVersionFile
      ];

    packages = lib.optional cfg.bundler.enable cfg.bundler.package ++ [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable lspPackage;

    env.BUNDLE_PATH = config.env.DEVENV_STATE + "/.bundle";

    env.GEM_HOME = "${config.env.BUNDLE_PATH}/${cfg.package.rubyEngine}/${cfg.package.version.libDir}";

    enterShell =
      let
        libdir = cfg.package.version.libDir;
      in
      ''
        export RUBYLIB="$DEVENV_PROFILE/${libdir}:$DEVENV_PROFILE/lib/ruby/site_ruby:$DEVENV_PROFILE/lib/ruby/site_ruby/${libdir}:$DEVENV_PROFILE/lib/ruby/site_ruby/${libdir}/${pkgs.stdenv.system}:''${RUBYLIB:-}"
        export GEM_PATH="$GEM_HOME/gems:''${GEM_PATH:-}"
        export PATH="$GEM_HOME/bin:$PATH"
      '';
  };
}
