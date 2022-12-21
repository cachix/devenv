{ pkgs, config, lib, inputs, ... }:

let
  inherit (lib.attrsets) attrValues getAttrs;
  cfg = config.languages.rust;
  setup = ''
    inputs:
      fenix:
        url: github:nix-community/fenix
        inputs:
          nixpkgs:
            follows: nixpkgs
  '';
in
{
  options.languages.rust = {
    enable = lib.mkEnableOption "Enable tools for Rust development.";

    packages = lib.mkOption {
      type = lib.types.attrsOf lib.types.package;
      default = { inherit (pkgs) rustc cargo rustfmt clippy rust-analyzer; };
      defaultText = "pkgs";
      description = "Attribute set of packages including rustc and cargo";
    };

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Set to stable, beta or latest.";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      packages = attrValues (getAttrs [ "rustc" "cargo" "rustfmt" "clippy" "rust-analyzer" ] cfg.packages);
      pre-commit.tools.cargo = lib.mkForce cfg.packages.cargo;
      pre-commit.tools.rustfmt = lib.mkForce cfg.packages.rustfmt;
      pre-commit.tools.clippy = lib.mkForce cfg.packages.clippy;
    })
    (lib.mkIf (cfg.version == null) {
      env.RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
    })
    (lib.mkIf (cfg.version != null) (
      let
        fenix = inputs.fenix or (throw "To use languages.rust.version, you need to add the following to your devenv.yaml:\n\n${setup}");
        rustPackages = fenix.packages.${pkgs.system}.${cfg.version} or (throw "languages.rust.version is set to ${cfg.version}, but should be one of: stable, beta or latest.");
      in
      {
        languages.rust.packages = rustPackages;
        env.RUST_SRC_PATH = "${inputs.fenix.packages.${pkgs.system}.${cfg.version}.rust-src}/lib/rustlib/src/rust/library";
      }
    ))
  ];
}
