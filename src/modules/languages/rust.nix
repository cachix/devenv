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

  tryPath = p: pkgs.lib.optional (pkgs.lib.pathExists p) p;
in
{
  options.languages.rust = {
    enable = lib.mkEnableOption "tools for Rust development";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "nixpkgs";
      default = pkgs.symlinkJoin {
        name = "nixpkgs-rust";
        paths = with pkgs; [
          rustc
          cargo
          rustfmt
          clippy
          rust-analyzer
        ];
      };
      description = "Rust package including rustc and Cargo.";
    };

    components = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" ];
      defaultText = lib.literalExpression ''[ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" ]'';
      description = ''
        List of [Rustup components](https://rust-lang.github.io/rustup/concepts/components.html)
        to install. Defaults to those available in `nixpkgs`.
      '';
    };

    rust-src = lib.mkOption {
      type = lib.types.path;
      default = pkgs.rustPlatform.rustLibSrc;
      defaultText = "`pkgs.rustPlatform.rustLibSrc` or `toolchain.rust-src`, depending on if a fenix toolchain is set.";
      description = ''
        The path to the rust-src Rustup component. Note that this is necessary for some tools
        like rust-analyzer to work. See [Rustup docs](https://rust-lang.github.io/rustup/concepts/components.html)
        for more information.
      '';
    };

    toolchain = lib.mkOption {
      # TODO: better type
      type = lib.types.nullOr (lib.types.attrsOf lib.types.anything);
      default = null;
      defaultText = lib.literalExpression "fenix.packages.stable";
      description = "The [fenix toolchain](https://github.com/nix-community/fenix#toolchain) to use.";
    };

    version = lib.mkOption {
      type = lib.types.enum [ null "stable" "beta" "latest" ];
      default = null;
      defaultText = lib.literalExpression "null";
      description = "The toolchain version to install.";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      packages = [ cfg.package ] ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

      # enable compiler tooling by default to expose things like cc
      languages.c.enable = lib.mkDefault true;

      env.RUST_SRC_PATH = cfg.rust-src;

      pre-commit.tools.cargo = lib.mkDefault pkgs.cargo;
      pre-commit.tools.rustfmt = lib.mkDefault pkgs.rustfmt;
      pre-commit.tools.clippy = lib.mkDefault pkgs.clippy;
    })
    (lib.mkIf (cfg.enable && pkgs.stdenv.isDarwin) {
      env.RUSTFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
      env.RUSTDOCFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
      env.CFLAGS = [ "-iframework ${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
    })
    (lib.mkIf (cfg.toolchain != null) {
      languages.rust.package = lib.mkForce (cfg.toolchain.withComponents cfg.components);
      languages.rust.rust-src = lib.mkForce "${cfg.toolchain.rust-src}/lib/rustlib/src/rust/library";

      pre-commit.tools.cargo = lib.mkForce cfg.toolchain.cargo;
      pre-commit.tools.clippy = lib.mkForce cfg.toolchain.clippy;
      pre-commit.tools.rustfmt = lib.mkForce cfg.toolchain.rustfmt;
    })
    (lib.mkIf (cfg.version != null) (
      let
        fenix = inputs.fenix or (throw "To use languages.rust.version, you need to add the following to your devenv.yaml:\n\n${setup}");
        rustPackages = fenix.packages.${pkgs.stdenv.system}.${cfg.version} or (throw "languages.rust.version is set to ${cfg.version}, but should be one of: stable, beta or latest.");
      in
      {
        languages.rust.toolchain = rustPackages;
      }
    ))
  ];
}
