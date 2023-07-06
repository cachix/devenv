{ pkgs, config, lib, inputs, ... }:

let
  cfg = config.languages.rust;
  setup = ''
    inputs:
      fenix:
        url: github:nix-community/fenix
        inputs:
          nixpkgs:
            follows: nixpkgs
  '';

  error = dbg: "To use languages.rust.${dbg}, you need to add the following to your devenv.yaml:\n\n${setup}";
in
{
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
    (lib.mkIf cfg.enable {
      packages = (builtins.map (c: cfg.toolchain.${c} or (throw (error "toolchain.${c}"))) cfg.components)
        ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

      # enable compiler tooling by default to expose things like cc
      languages.c.enable = lib.mkDefault true;

      # RUST_SRC_PATH is necessary when rust-src is not at the same location as
      # as rustc. This is the case with the rust toolchain from nixpkgs.
      env.RUST_SRC_PATH =
        if cfg.toolchain ? rust-src
        then "${cfg.toolchain.rust-src}/lib/rustlib/src/rust/library"
        else pkgs.rustPlatform.rustLibSrc;

      pre-commit.tools.cargo = cfg.toolchain.cargo or null;
      pre-commit.tools.rustfmt = cfg.toolchain.rustfmt or null;
      pre-commit.tools.clippy = cfg.toolchain.clippy or null;
    })
    (lib.mkIf (cfg.enable && pkgs.stdenv.isDarwin) {
      env.RUSTFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
      env.RUSTDOCFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
      env.CFLAGS = [ "-iframework ${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
    })
    (lib.mkIf (cfg.channel != "nixpkgs") (
      let
        err = error "channel";
        fenix = inputs.fenix or (throw err);
        rustPackages = fenix.packages.${pkgs.stdenv.system} or (throw err);
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
