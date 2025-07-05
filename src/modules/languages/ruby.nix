{ pkgs, config, lib, ... }:

let
  cfg = config.languages.ruby;

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

    bundler = {
      enable = lib.mkEnableOption "bundler";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.bundler.override { ruby = cfg.package; };
        defaultText = lib.literalExpression "pkgs.bundler.override { ruby = cfg.package; }";
        description = "The bundler package to use.";
      };
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Ruby development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Ruby language server (solargraph).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.rubyPackages.solargraph;
          defaultText = lib.literalExpression "pkgs.rubyPackages.solargraph";
          description = ''
            The Ruby language server package to use.
            
            Available options:
            - `pkgs.rubyPackages.solargraph` (default): Mature, feature-rich LSP by Fred Snyder
            - `pkgs.rubyPackages.ruby-lsp`: Newer LSP by Shopify, actively developed
            
            To switch to ruby-lsp, use:
            ```nix
            languages.ruby.dev.lsp.package = pkgs.rubyPackages.ruby-lsp;
            ```
          '';
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Ruby formatter (rubocop).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.rubyPackages.rubocop;
          defaultText = lib.literalExpression "pkgs.rubyPackages.rubocop";
          description = "The rubocop package to use.";
        };
      };

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Ruby linter (rubocop).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.rubyPackages.rubocop;
          defaultText = lib.literalExpression "pkgs.rubyPackages.rubocop";
          description = "The rubocop package to use.";
        };
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
          nixpkgs-ruby.packages.${pkgs.stdenv.system}."ruby-${cfg.version}"
        );
        packageFromVersionFile = lib.mkIf (cfg.versionFile != null) (
          nixpkgs-ruby.lib.packageFromRubyVersionFile {
            file = cfg.versionFile;
            system = pkgs.stdenv.system;
          }
        );
      in
      lib.mkMerge [
        packageFromVersion
        packageFromVersionFile
      ];

    packages = [
      cfg.package
    ] ++ lib.optional cfg.bundler.enable cfg.bundler.package
    ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package ++
        lib.optional cfg.dev.linter.enable cfg.dev.linter.package
    );

    env.BUNDLE_PATH = config.env.DEVENV_STATE + "/.bundle";

    env.GEM_HOME = "${config.env.BUNDLE_PATH}/${cfg.package.rubyEngine}/${cfg.package.version.libDir}";

    enterShell =
      let libdir = cfg.package.version.libDir;
      in
      ''
        export RUBYLIB="$DEVENV_PROFILE/${libdir}:$DEVENV_PROFILE/lib/ruby/site_ruby:$DEVENV_PROFILE/lib/ruby/site_ruby/${libdir}:$DEVENV_PROFILE/lib/ruby/site_ruby/${libdir}/${pkgs.stdenv.system}:''${RUBYLIB:-}"
        export GEM_PATH="$GEM_HOME/gems:''${GEM_PATH:-}"
        export PATH="$GEM_HOME/bin:$PATH"
      '';
  };
}
