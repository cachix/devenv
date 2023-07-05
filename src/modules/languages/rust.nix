{ pkgs, config, lib, inputs, ... }:

let
  cfg = config.languages.rust;
  setup = ''
    inputs:
      fenix:
        url: github:nix-community/fenix
        inputs:
          nixpkgs:
            follow: nixpkgs
  '';

  fenix' = inputs.fenix or
    (throw "to use languages.rust, you must add the following to your devenv.yaml:\n\n${setup}");
  fenix = fenix'.packages.${pkgs.stdenv.system};
in
{
  options.languages.rust = {
    enable = lib.mkEnableOption "tools for Rust development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Rust package including rustc and Cargo.";
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
    };

    components = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      description = ''
        List of [Rustup components](https://rust-lang.github.io/rustup/concepts/components.html)
        to install.
      '';
      default = [
        "rustc"
        "cargo"
        "clippy"
        "rustfmt"
        "rust-analyzer"
      ];
      defaultText = lib.literalExpression ''[ "rust-analyzer" ]'';
    };

    rust-src = lib.mkOption {
      type = lib.types.path;
      default = pkgs.rustPlatform.rustLibSrc;
    };

    toolchain = lib.mkOption {
      # TODO: better type with https://nixos.org/manual/nixos/stable/index.html
      type = lib.types.nullOr lib.types.anything;
      description = ''
        The [fenix toolchain](https://github.com/nix-community/fenix#toolchain) to use.
      '';
      default = null;
      defaultText = lib.literalExpression "fenix.packages.stable";
    };

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      description = ''
        The [Rustup channel](https://rust-lang.github.io/rustup/concepts/channels.html) to install.
      '';
      default = null;
      defaultText = lib.literalExpression "null";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      packages = [ cfg.package ]
        ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

      env.RUST_SRC_PATH = cfg.rust-src;

      # enable compiler tooling by default to expose things like cc
      languages.c.enable = lib.mkDefault true;
    })
    (lib.mkIf (cfg.toolchain != null) {
      languages.rust.package = lib.mkForce
        (cfg.toolchain.withComponents cfg.components);

      languages.rust.rust-src = lib.mkForce "${cfg.toolchain.rust-src}/lib/rustlib/src/rust/library";
    })
    (lib.mkIf (cfg.version != null) {
      languages.rust.toolchain = lib.mkForce (fenix.${cfg.version});
    })
  ];
}
