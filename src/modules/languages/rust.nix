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

  fenix-input = dbg: inputs.fenix or (throw "To use languages.rust.${dbg}, you need to add the following to your devenv.yaml:\n\n${setup}");

  mkFenixPackage = toolchain:
    toolchain.combine
      (map
        (component:
          if component == "rust-src"
          then "${toolchain.rust-src}/lib/rustlib/src/rust/library"
          else toolchain.${component})
        cfg.components);
in
{
  options.languages.rust = {
    enable = lib.mkEnableOption "tools for Rust development";

    components = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "rustc" "cargo" "rustfmt" "clippy" "rust-analyzer" "rust-src" ];
      defaultText = lib.literalExpression ''[ "rustc" "cargo" "rustfmt" "clippy" "rust-analyzer" "rust-src" ]'';
      description = "Rust components to install.";
    };

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "pkgs";
      default = pkgs.symlinkJoin {
        name = "rust-pkgs";
        paths = map
          (component:
            if component == "rust-src"
            then pkgs.rustPlatform.rustLibSrc
            else pkgs.${component})
          cfg.components;
      };
      description = "Rust package including rustc and Cargo.";
    };

    toolchain = lib.mkOption {
      type = lib.types.nullOr ((fenix-input "toolchain").packages.${pkgs.stdenv.system}.stable).type;
      default = null;
      description = ''
        The [fenix toolchain](https://github.com/nix-community/fenix#toolchain) to use.

        To use fenix, add the following to your devenv.yaml:
        ```yaml title="devenv.yaml"
        ${setup}
        ```
      '';
      defaultText = lib.literalExpression "null";
    };

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Set to stable, beta, or latest.";
      defaultText = "null";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      packages = [ cfg.package ] ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

      # enable compiler tooling by default to expose things like cc
      languages.c.enable = lib.mkDefault true;

      pre-commit.tools.cargo = lib.mkForce cfg.packages.cargo;
      pre-commit.tools.rustfmt = lib.mkForce cfg.packages.rustfmt;
      pre-commit.tools.clippy = lib.mkForce cfg.packages.clippy;
    })
    (lib.mkIf (cfg.enable && pkgs.stdenv.isDarwin) {
      env.RUSTFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
      env.RUSTDOCFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
      env.CFLAGS = [ "-iframework ${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
    })
    (lib.mkIf (cfg.toolchain != null) {
      languages.rust.package = mkFenixPackage cfg.toolchain;
    })
    (lib.mkIf (cfg.version != null) (
      let
        fenix = fenix-input "version";
        fenixPackages = fenix.packages.${pkgs.stdenv.system};
        toolchain = fenixPackages.${cfg.version} or (throw "languages.rust.version is set to ${cfg.version}, but should be one of: stable, beta or latest.");
      in
      {
        languages.rust.package = mkFenixPackage toolchain;
      }
    ))
    (lib.mkIf ((cfg.toolchain != null) && (cfg.version != null)) (
      throw "languages.rust.toolchain and languages.rust.version are both set. Please set only one of them."
    ))
  ];
}
