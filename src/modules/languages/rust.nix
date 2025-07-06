{ pkgs, config, lib, ... }:

let
  cfg = config.languages.rust;

  rust-overlay = config.lib.getInput {
    name = "rust-overlay";
    url = "github:oxalica/rust-overlay";
    attribute = "languages.rust.input";
    follows = [ "nixpkgs" ];
  };
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "languages" "rust" "packages" ] [ "languages" "rust" "toolchain" ])
  ];

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

    targets = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      defaultText = lib.literalExpression ''[ ]'';
      description = ''
        List of extra [targets](https://doc.rust-lang.org/nightly/rustc/platform-support.html)
        to install. Defaults to the native target.
      '';
    };

    channel = lib.mkOption {
      type = lib.types.enum [ "nixpkgs" "stable" "beta" "nightly" ];
      default = "nixpkgs";
      description = ''
        The rustup toolchain to install.

        `nixpkgs` is a special channel.
        It will use whichever version is currently available in nixpkgs.
      '';
    };

    version = lib.mkOption {
      type = lib.types.str;
      default = "latest";
      description = ''
        The version of rust to use.

        Examples: `latest`,`1.81.0`, `2021-01-01`.

        Only used when languages.rust.channel is NOT set to `nixpkgs`.
      '';
    };

    profile = lib.mkOption {
      type = lib.types.enum [ "default" "minimal" "complete" ];
      default = "default";
      description = ''
        The rustup toolchain [profile](https://rust-lang.github.io/rustup/concepts/profiles.html) to use.

        Only used when languages.rust.channel is NOT set to `nixpkgs`.
      '';
    };

    rustflags = lib.mkOption {
      type = lib.types.str;
      default = "";
      description = "Extra flags to pass to the Rust compiler.";
    };

    mold.enable = lib.mkOption {
      type = lib.types.bool;
      default = pkgs.stdenv.isLinux && pkgs.stdenv.isx86_64 && cfg.targets == [ ];
      defaultText =
        lib.literalExpression "pkgs.stdenv.isLinux && pkgs.stdenv.isx86_64 && languages.rust.targets == [ ]";
      description = ''
        Enable mold as the linker.

        Enabled by default on x86_64 Linux machines when no cross-compilation targets are specified.
      '';
    };

    # Read-only components of the toolchain.
    # toolchainComponents = ...

    toolchain = lib.mkOption {
      type = lib.types.package;
      description = ''
        The Rust toolchain to use.

        When the channel is set to `nixpkgs`, the toolchain is created by symlinking the individual components from `languages.rust.components`.

        For other channels, the toolchain is created using rust-overlay with the specified version, profile, and components.
      '';
    };

    rustBin = lib.mkOption {
      type = lib.types.nullOr lib.types.anything;
      readOnly = true;
      default = null;
      description = ''
        Initialized rust-overlay library.

        Only available when `channel` is not set to `nixpkgs`.
      '';
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    (
      let
        mkOverrideTools = lib.mkOverride (lib.modules.defaultOverridePriority - 1);
      in
      {
        assertions = [
          {
            assertion = cfg.channel == "nixpkgs" -> (cfg.targets == [ ]);
            message = ''
              Cannot use `languages.rust.channel = "nixpkgs"` with `languages.rust.targets`.

              The nixpkgs channel does not support cross-compiling with targets.
              Use the stable, beta, or nightly channels instead. For example:

              languages.rust.channel = "stable";
            '';
          }
          {
            assertion = cfg.channel == "nixpkgs" -> (cfg.version == "latest");
            message = ''
              Cannot use `languages.rust.channel = "nixpkgs"` with `languages.rust.version`.

              The nixpkgs channel does not contain all versions required, and is
              therefore not supported to be used together.

              languages.rust.channel = "stable";
            '';
          }
        ];

        # Set $CARGO_INSTALL_ROOT so that executables installed by `cargo install` can be found from $PATH
        enterShell = ''
          export CARGO_INSTALL_ROOT=$(${
            lib.strings.escapeShellArgs [
              "${pkgs.coreutils}/bin/realpath"
              "--no-symlinks"
              "${config.devenv.state}/cargo-install"
            ]
          })
          export PATH="$PATH:$CARGO_INSTALL_ROOT/bin"
        '';

        packages =
          lib.optional cfg.mold.enable pkgs.mold-wrapped
          ++ lib.optional pkgs.stdenv.isDarwin pkgs.libiconv;

        # enable compiler tooling by default to expose things like cc
        languages.c.enable = lib.mkDefault true;

        env =
          let
            moldFlags = lib.optionalString cfg.mold.enable "-C link-arg=-fuse-ld=mold";
            optionalEnv = cond: str: if cond then str else null;
          in
          {
            RUSTFLAGS = optionalEnv (moldFlags != "" || cfg.rustflags != "") (lib.concatStringsSep " " (lib.filter (x: x != "") [ moldFlags cfg.rustflags ]));
            RUSTDOCFLAGS = optionalEnv (moldFlags != "") moldFlags;
            CFLAGS = lib.optionalString pkgs.stdenv.isDarwin "-iframework ${config.devenv.profile}/Library/Frameworks";
          };

        git-hooks.tools.cargo = mkOverrideTools cfg.toolchain;
        git-hooks.tools.rustfmt = mkOverrideTools cfg.toolchain;
        git-hooks.tools.clippy = mkOverrideTools cfg.toolchain;
      }
    )

    (lib.mkIf (cfg.channel == "nixpkgs") {
      languages.rust.toolchain = pkgs.symlinkJoin {
        name = "nixpkgs-rust-toolchain";
        paths = builtins.map
          (c:
            if c == "rust-src"
            then pkgs.rustPlatform.rustcSrc
            else
              pkgs.${c} or (throw "No rust component named ${c} in pkgs"))
          cfg.components;
      };
      packages = [ cfg.toolchain ];
    })

    (lib.mkIf (cfg.channel != "nixpkgs") (
      let
        rustBin = rust-overlay.lib.mkRustBin { } pkgs.buildPackages;

        # Get the pre-made toolchain for the channel and version
        baseToolchain = rustBin.${cfg.channel}.${cfg.version};

        # Get the combined toolchain for the specified profile with overrides
        combinedToolchain = baseToolchain.${cfg.profile}.override {
          extensions = cfg.components;
          targets = cfg.targets;
        };
      in
      {
        languages.rust.rustBin = rustBin;
        languages.rust.toolchain = combinedToolchain;
        packages = [ cfg.toolchain ];
      }
    ))
  ]);
}
