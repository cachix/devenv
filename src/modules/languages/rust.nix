{ pkgs, config, lib, inputs, ... }:

let
  cfg = config.languages.rust;
  setup = ''
    inputs:
      rust-overlay:
        url: github:oxalica/rust-overlay
        inputs:
          nixpkgs:
            follows: nixpkgs
  '';
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
      type = lib.types.enum [ null "stable" "beta" "nightly" ];
      default = null;
      defaultText = lib.literalExpression "null";
      description = "The rustup toolchain to install.";
    };

    toolchain = lib.mkOption {
      type = lib.types.submodule ({
        freeformType = lib.types.attrsOf lib.types.package;

        options = {
          rust-src = lib.mkOption {
            type = lib.types.path;
            default = pkgs.rustPlatform.rustLibSrc;
            defaultText = lib.literalExpression "pkgs.rustPlatform.rustLibSrc";
            description = "rust-src package";
          };
        } // (
          let
            documented-components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" ];
            mkComponentOption = component: lib.mkOption {
              type = lib.types.nullOr lib.types.package;
              default = pkgs.${component};
              defaultText = lib.literalExpression "pkgs.${component}";
              description = "${component} package";
            };
          in
          lib.genAttrs documented-components mkComponentOption
        );
      });
      defaultText = lib.literalExpression "fenix.packages.stable";
      description = "The location of every component to use.";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      packages = (lib.getAttrs cfg.components cfg.toolchain)
        ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

      # enable compiler tooling by default to expose things like cc
      languages.c.enable = lib.mkDefault true;

      # RUST_SRC_PATH is necessary when rust-src is not at the same location as
      # as rustc. This is the case with the rust toolchain from nixpkgs.
      env.RUST_SRC_PATH = cfg.toolchain.rust-src;

      pre-commit.tools.cargo = cfg.toolchain.cargo;
      pre-commit.tools.rustfmt = cfg.toolchain.rustfmt;
      pre-commit.tools.clippy = cfg.toolchain.clippy;
    })
    (lib.mkIf (cfg.enable && pkgs.stdenv.isDarwin) {
      env.RUSTFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
      env.RUSTDOCFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
      env.CFLAGS = [ "-iframework ${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
    })
    (lib.mkIf (cfg.channel != null) (
      let
        error = "To use languages.rust.version, you need to add the following to your devenv.yaml:\n\n${setup}";
        rust-overlay = inputs.rust-overlay or (throw error);
        rustPackages = rust-overlay.packages.${pkgs.stdenv.system} or (throw error);
      in
      {
        languages.rust.toolchain =
          if cfg.channel == "stable"
          then rustPackages.rust
          else rustPackages."rust-${cfg.channel}";
      }
    ))
  ];
}
