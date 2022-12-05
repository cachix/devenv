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
in
{
  options.languages.rust = {
    enable = lib.mkEnableOption "Enable tools for Rust development.";

    packages = lib.mkOption {
      type = lib.types.attrsOf lib.types.package;
      default = { inherit (pkgs) rustc cargo; };
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
      packages = [
        cfg.packages.rustc
        cfg.packages.cargo
      ];

      enterShell = ''
        rustc --version
        cargo --version
      '';
    })
    (lib.mkIf (cfg.version != null) (
      let
        fenix = inputs.fenix or (throw "To use languages.rust.version, you need to add the following to your devenv.yaml:\n\n${setup}");
        rustPackages = fenix.packages.${pkgs.system}.${cfg.version} or (throw "languages.rust.version is set to ${cfg.version}, but should be one of: stable, beta or latest.");
      in
      {
        languages.rust.packages = rustPackages;

        pre-commit.tools.cargo = lib.mkForce rustPackages.cargo;
        pre-commit.tools.rustfmt = lib.mkForce rustPackages.rustfmt;
        pre-commit.tools.clippy = lib.mkForce rustPackages.clippy;
      }
    ))
  ];
}
