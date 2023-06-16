{ pkgs, config, lib, inputs, ... }:

let
  inherit (lib.attrsets) nameValuePair;

  cfg = config.languages.rust;
  setup = ''
    inputs:
      fenix:
        url: github:nix-community/fenix
        inputs:
          nixpkgs:
            follows: nixpkgs
  '';

  get-nixpkgs-component = component:
    if component == "rust-src" then
      nameValuePair
        "pkgs.rustPlatform.rustLibSrc"
        pkgs.rustPlatform.rustLibSrc
    else
      nameValuePair
        "pkgs.fenix.components.${component}"
        pkgs.fenix.components.${component};
in
{
  options.languages.rust2 = {
    enable = lib.mkEnableOption "tools for Rust development";

    packages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      defaultText = lib.literalExpression "[ ]";
      description = "Packages to install";
    };

    toolchain = lib.mkOption {
      type =
        lib.types.coercedTo
          lib.types.anything
          (c: c.withComponents)
          (lib.types.functionTo lib.types.package);

      default = {
        withComponents = components: pkgs.symlinkJoin "rust-components" (
          builtins.map get-nixpkgs-component components
        );
      };

      defaultText = lib.literalExpression ''{
        withComponents = components: pkgs.symlinkJoin "rust-components" (
          builtins.map get-nixpkgs-component components
        );
      }'';

      description = "Toolchain object that provides a ${lib.literalExpression "withComponents"} function to create the complete toolchain.";
    };

    components = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "rustc" "cargo" "rustfmt" "clippy" "rust-analyzer" ];
      defaultText = lib.literalExpression ''[ "rustc" "cargo" "rustfmt" "clippy" "rust-analyzer" ]'';
      description = "Rust components to install";
    };

    targets = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ pkgs.stdenv.system ];
      defaultText = lib.literalExpression ''[ pkgs.stdenv.system ]'';
      description = "Rust targets to install";
    };

    channel = lib.mkOption {
      type = lib.types.nullOr (lib.types.either lib.types.str (lib.types.submodule {
        options = {
          name = lib.mkOption {
            type = lib.types.str;
            description = "Name of the channel";
          };

          sha256 = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            defaultText = lib.literalExpression "null";
            description = ''SHA256 of the channel manifest file.

            This is used to verify the channel manifest file. If not specified, the
            channel manifest file will not be verified but will still work. This is
            useful for optimizing Nix evaluation, since not specifying the SHA256
            will result in [an import from derivation](https://nixos.wiki/wiki/Import_From_Derivation).'';
          };
        };
      }));
      default = null;
      defaultText = lib.literalExpression "null";
      description = "Rust channel to use";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      packages = cfg.packages
        ++ cfg.toolchain cfg.components
        ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

      # enable compiler tooling by default to expose things like cc
      languages.c.enable = lib.mkDefault true;

      env.RUST_SRC_PATH = cfg.packages.rust-src;

      pre-commit.tools.cargo = lib.mkForce cfg.packages.cargo;
      pre-commit.tools.rustfmt = lib.mkForce cfg.packages.rustfmt;
      pre-commit.tools.clippy = lib.mkForce cfg.packages.clippy;
    })
  ];
}
