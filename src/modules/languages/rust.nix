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
    enable = lib.mkEnableOption "Enable tools for Rust development.";

    packages = lib.mkOption {
      type = lib.types.submodule ({ ... }: {
        options = genAttrs tools (name: lib.mkOption {
          type = lib.types.package;
          default = pkgs.${name};
          defaultText = "pkgs.${name}";
          description = "${name} package";
        });
      });
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
      packages = attrValues (getAttrs tools cfg.packages);
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
        languages.rust.packages = genAttrs tools (package: lib.mkDefault rustPackages.${package});
        env.RUST_SRC_PATH = "${inputs.fenix.packages.${pkgs.system}.${cfg.version}.rust-src}/lib/rustlib/src/rust/library";
      }
    ))
  ];
}
