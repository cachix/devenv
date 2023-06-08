{ pkgs
, config
, lib
, inputs
, ...
}:
let
  inherit (lib.attrsets) attrValues genAttrs getAttrs;
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
    enable = lib.mkEnableOption "tools for Rust development";
    toolchain = lib.mkOption {
      type = lib.types.submodule {
        options = {
          channel = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            description = ''null|"stable"|"beta"|"nightly"|"<major.minor.patch>"'';
            example = "nightly";
          };
          date = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            description = ''null|"YYYY-MM-DD". Has no effect if `channel` is unset.'';
            example = "2023-01-31";
          };
          profile = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            description = ''null|"minimal"|"default"|"complete"'';
            example = "minimal";
          };
          components = lib.mkOption {
            type = lib.types.nullOr (lib.types.listOf lib.types.str);
            default = null;
            description = ''See https://rust-lang.github.io/rustup/concepts/components.html for a list of valid components.'';
            example = [ "rust-src" "rust-analyzer" ];
          };
          targets = lib.mkOption {
            type = lib.types.nullOr (lib.types.listOf lib.types.str);
            default = null;
            description = "See https://github.com/nix-community/fenix#supported-platforms-and-targets for a list of valid targets.";
            example = [ "wasm32-unknown-unknown" ];
          };
        };
      };
      default = { };
      description = "Attribute set of toolchain file values. See https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file for more information.";
    };
  };
  config = lib.mkMerge [
    (lib.mkIf cfg.enable (
      let
        fenix = inputs.fenix.packages.${pkgs.stdenv.system} or (throw "To use languages.rust, you need to add the following to your devenv.yaml:\n\n${setup}");
        listify = lib.concatMapStringsSep "," (a: ''"${a}"'');
        toolchain_toml =
          if cfg.toolchain == null
          then builtins.toFile "rust-toolchain.toml" ''[toolchain]''
          else
            let
              channel =
                if cfg.toolchain.channel != null && cfg.toolchain.channel != ""
                then ''channel = "${cfg.toolchain.channel
                + (
                  if cfg.toolchain.date != null && cfg.toolchain.date != ""
                  then "-" + cfg.toolchain.date
                  else ""
                )}"''
                else ''channel = "stable"'';
              components =
                if cfg.toolchain.components != null && lib.length cfg.toolchain.components > 0
                then ''components = [${listify cfg.toolchain.components}]''
                else "";
              targets =
                if cfg.toolchain.targets != null && lib.length cfg.toolchain.targets > 0
                then ''targets = [${listify cfg.toolchain.targets}]''
                else "";
              profile =
                if cfg.toolchain.profile != null && cfg.toolchain.profile != ""
                then ''profile = "${cfg.toolchain.profile}"''
                else "";
            in
            builtins.toFile "rust-toolchain.toml" ''[toolchain]
${channel}
${components}
${targets}
${profile}
'';
        toolchain_derivation = fenix.fromToolchainFile {
          file = toolchain_toml;
          sha256 = null;
        };
      in
      {
        packages = [ toolchain_derivation ] ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;
        # enable compiler tooling by default to expose things like cc
        languages.c.enable = lib.mkDefault true;
        pre-commit.tools.cargo = lib.mkForce toolchain_derivation;
        # Use the packages from the toolchain derivation if it contains them, otherwise use default packages.
        pre-commit.tools.clippy =
          if cfg.toolchain.profile == "default" || cfg.toolchain.profile == "complete" || (cfg.toolchain.components != null && builtins.elem "clippy" cfg.toolchain.components)
          then lib.mkForce toolchain_derivation
          else lib.mkForce pkgs.clippy;
        pre-commit.tools.rustfmt =
          if cfg.toolchain.profile == "default" || cfg.toolchain.profile == "complete" || (cfg.toolchain.components != null && builtins.elem "rustfmt" cfg.toolchain.components)
          then lib.mkForce toolchain_derivation
          else lib.mkForce pkgs.rustfmt;
      }
    ))
    (lib.mkIf (cfg.enable && pkgs.stdenv.isDarwin) {
      env.RUSTFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
      env.RUSTDOCFLAGS = [ "-L framework=${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
      env.CFLAGS = [ "-iframework ${config.env.DEVENV_PROFILE}/Library/Frameworks" ];
    })
  ];
}
