{ pkgs, config, lib, ... }:

let
  cfg = config.languages.rust;

  fenix = config.lib.getInput {
    name = "fenix";
    url = "github:nix-community/fenix";
    attribute = "languages.rust.version";
    follows = [ "nixpkgs" ];
  };
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "languages" "rust" "version" ] [ "languages" "rust" "channel" ])
    (lib.mkRenamedOptionModule [ "languages" "rust" "packages" ] [ "languages" "rust" "toolchain" ])
  ];

  options.languages.rust = {
    enable = lib.mkEnableOption "tools for Rust development";

    components = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" ];
      defaultText = lib.literalExpression ''[ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" ]'';
      description = ''
        List of [Rustup components](https://rust-lang.github.io/rustup/concepts/components.html)
        to install. Defaults to those available in `nixpkgs`.
      '';
    };

    channel = lib.mkOption {
      type = lib.types.enum [ "nixpkgs" "stable" "beta" "nightly" ];
      default = "nixpkgs";
      defaultText = lib.literalExpression ''"nixpkgs"'';
      description = "The rustup toolchain to install.";
    };

    toolchain = lib.mkOption {
      type = lib.types.submodule ({
        freeformType = lib.types.attrsOf lib.types.package;

        options =
          let
            documented-components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" ];
            mkComponentOption = component: lib.mkOption {
              type = lib.types.nullOr lib.types.package;
              default = pkgs.${component};
              defaultText = lib.literalExpression "pkgs.${component}";
              description = "${component} package";
            };
          in
          lib.genAttrs documented-components mkComponentOption;
      });
      default = { };
      defaultText = lib.literalExpression "nixpkgs";
      description = "Rust component packages. May optionally define additional components, for example `miri`.";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable (
      let
        mkOverrideTools = lib.mkOverride (lib.modules.defaultOverridePriority - 1);
      in
      {
        # Set $CARGO_INSTALL_ROOT so that executables installed by `cargo install` can be found from $PATH
        enterShell = ''
          export CARGO_INSTALL_ROOT=$(${
            lib.strings.escapeShellArgs [
              "${pkgs.coreutils}/bin/realpath"
              "--no-symlinks"
              "${config.devenv.state}/cargo-install"
            ]
          })
          export PATH="$PATH:$CARGO_INSTALL_ROOT/bin"
        '';

        packages = (builtins.map (c: cfg.toolchain.${c} or (throw "toolchain.${c}")) cfg.components)
          ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

        # enable compiler tooling by default to expose things like cc
        languages.c.enable = lib.mkDefault true;

        # RUST_SRC_PATH is necessary when rust-src is not at the same location as
        # as rustc. This is the case with the rust toolchain from nixpkgs.
        env.RUST_SRC_PATH =
          if cfg.toolchain ? rust-src
          then "${cfg.toolchain.rust-src}/lib/rustlib/src/rust/library"
          else pkgs.rustPlatform.rustLibSrc;

        pre-commit.tools.cargo = mkOverrideTools cfg.toolchain.cargo or null;
        pre-commit.tools.rustfmt = mkOverrideTools cfg.toolchain.rustfmt or null;
        pre-commit.tools.clippy = mkOverrideTools cfg.toolchain.clippy or null;
      }
    ))
    (lib.mkIf (cfg.enable && pkgs.stdenv.isDarwin) {
      env.RUSTFLAGS = "-L framework=${config.devenv.profile}/Library/Frameworks";
      env.RUSTDOCFLAGS = "-L framework=${config.devenv.profile}/Library/Frameworks";
      env.CFLAGS = "-iframework ${config.devenv.profile}/Library/Frameworks";
    })
    (lib.mkIf (cfg.channel != "nixpkgs") (
      let
        rustPackages = fenix.packages.${pkgs.stdenv.system};
      in
      {
        languages.rust.toolchain =
          let
            toolchain =
              if cfg.channel == "nightly"
              then rustPackages.latest
              else rustPackages.${cfg.channel};
          in
          (builtins.mapAttrs (_: pkgs.lib.mkDefault) toolchain);
      }
    ))
  ];
}
