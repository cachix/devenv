{ pkgs, config, lib, inputs, ... }:

let
  inherit (lib.attrsets) attrValues genAttrs getAttrs;

  cfg = config.languages.rust;
  tools = [ "rustc" "cargo" "rustfmt" "clippy" "rust-analyzer" ];
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
    enable = lib.mkEnableOption "tools for Rust development";

    packages = lib.mkOption {
      type = lib.types.submodule ({
        options = {
          rust-src = lib.mkOption {
            type = lib.types.either lib.types.package lib.types.str;
            default = pkgs.rustPlatform.rustLibSrc;
            defaultText = lib.literalExpression "pkgs.rustPlatform.rustLibSrc";
            description = "rust-src package";
          };
        }
        // genAttrs tools (name: lib.mkOption {
          type = lib.types.package;
          default = pkgs.${name};
          defaultText = lib.literalExpression "pkgs.${name}";
          description = "${name} package";
        });
      });
      defaultText = lib.literalExpression "pkgs";
      default = { };
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
      packages = attrValues (getAttrs tools cfg.packages) ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

      # enable compiler tooling by default to expose things like cc
      languages.c.enable = lib.mkDefault true;

      env.RUST_SRC_PATH = cfg.packages.rust-src;

      pre-commit.tools.cargo = lib.mkForce cfg.packages.cargo;
      pre-commit.tools.rustfmt = lib.mkForce cfg.packages.rustfmt;
      pre-commit.tools.clippy = lib.mkForce cfg.packages.clippy;
    })
    (lib.mkIf (cfg.enable && pkgs.stdenv.isDarwin) {
      env.RUSTFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
      env.RUSTDOCFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
    })
    (lib.mkIf (cfg.version != null) (
      let
        fenix = inputs.fenix or (throw "To use languages.rust.version, you need to add the following to your devenv.yaml:\n\n${setup}");
        rustPackages = fenix.packages.${pkgs.system}.${cfg.version} or (throw "languages.rust.version is set to ${cfg.version}, but should be one of: stable, beta or latest.");
      in
      {
        languages.rust.packages =
          { rust-src = lib.mkDefault "${rustPackages.rust-src}/lib/rustlib/src/rust/library"; }
          // genAttrs tools (package: lib.mkDefault rustPackages.${package});
      }
    ))
  ];
}
