{ pkgs, config, lib, ... }:

let
  cfg = config.languages.rust;

  validChannels = [ "nixpkgs" "stable" "beta" "nightly" ];

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
        to install. Defaults to only the native target. 
      '';
    };

    channel = lib.mkOption {
      type = lib.types.enum validChannels;
      default = "nixpkgs";
      defaultText = lib.literalExpression ''"nixpkgs"'';
      description = "The rustup toolchain to install.";
    };

    version = lib.mkOption {
      type = lib.types.str;
      default = "latest";
      defaultText = lib.literalExpression ''"latest"'';
      description = ''
        Which version of rust to use, this value could be `latest`,`1.81.0`, `2021-01-01`.
        Only works when languages.rust.channel is NOT nixpkgs.
      '';
    };

    rustflags = lib.mkOption {
      type = lib.types.str;
      default = "";
      description = "Extra flags to pass to the Rust compiler.";
    };

    mold.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Use [mold](https://github.com/rui314/mold) as the linker.

        mold is a faster drop-in replacement for existing Unix linkers.
        It is several times quicker than the LLVM lld linker.
      '';
    };

    toolchain = lib.mkOption {
      type = lib.types.submodule ({
        freeformType = lib.types.attrsOf lib.types.package;

        options =
          let
            documented-components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" ];
            mkComponentOption = component: lib.mkOption {
              type = lib.types.nullOr lib.types.package;
              default = pkgs.${component};
              defaultText = lib.literalExpression "pkgs.${component}";
              description = "${component} package";
            };
          in
          lib.genAttrs documented-components mkComponentOption;
      });
      default = { };
      defaultText = lib.literalExpression "nixpkgs";
      description = "Rust component packages. May optionally define additional components, for example `miri`.";
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
            # RUST_SRC_PATH is necessary when rust-src is not at the same location as
            # as rustc. This is the case with the rust toolchain from nixpkgs.
            RUST_SRC_PATH =
              if cfg.toolchain ? rust-src
              then "${cfg.toolchain.rust-src}/lib/rustlib/src/rust/library"
              else pkgs.rustPlatform.rustLibSrc;
            RUSTFLAGS = optionalEnv (moldFlags != "" || cfg.rustflags != "") (lib.concatStringsSep " " (lib.filter (x: x != "") [ moldFlags cfg.rustflags ]));
            RUSTDOCFLAGS = optionalEnv (moldFlags != "") moldFlags;
            CFLAGS = lib.optionalString pkgs.stdenv.isDarwin "-iframework ${config.devenv.profile}/Library/Frameworks";
          };

        git-hooks.tools.cargo = mkOverrideTools cfg.toolchain.cargo or null;
        git-hooks.tools.rustfmt = mkOverrideTools cfg.toolchain.rustfmt or null;
        git-hooks.tools.clippy = mkOverrideTools cfg.toolchain.clippy or null;
      }
    )

    (lib.mkIf (cfg.channel == "nixpkgs") {
      packages = builtins.map (c: cfg.toolchain.${c} or (throw "toolchain.${c}")) cfg.components;
    })

    (lib.mkIf (cfg.channel != "nixpkgs") (
      let
        rustBin = rust-overlay.lib.mkRustBin { } pkgs.buildPackages;

        # WARNING: private API
        # Import the mkAggregated function.
        # This symlinkJoins and patches the individual components.
        mkAggregated = import (rust-overlay + "/lib/mk-aggregated.nix") {
          inherit (pkgs) lib stdenv symlinkJoin bash curl;
          inherit (pkgs.buildPackages) rustc;
          pkgsTargetTarget = pkgs.targetPackages;
        };

        # Get the toolchain for component resolution with error handling
        channel = rustBin.${cfg.channel} or (throw "Invalid Rust channel '${cfg.channel}'. Available: ${lib.concatStringsSep ", " (lib.filter (c: c != "nixpkgs") validChannels)}");
        toolchain = channel.${cfg.version} or (throw "Invalid Rust version '${cfg.version}' for channel '${cfg.channel}'. Available: ${lib.concatStringsSep ", " (builtins.attrNames channel)}");
        # A list of all available components. This will be filtered down to the requested components.
        availableComponents = toolchain._manifest.profiles.complete or [ ];

        # Try the component name, then with the -preview suffix.
        # rust-overlay has a more specific list of renames, but they're all just -preview differences.
        resolveComponentName = c:
          if builtins.elem c availableComponents then c
          else if builtins.elem "${c}-preview" availableComponents then "${c}-preview"
          else throw "Component '${c}' not found. Available: ${lib.concatStringsSep ", " availableComponents}";

        toolchainComponents = lib.filterAttrs (c: _: builtins.elem c availableComponents) toolchain;

        # Resolve components with user overrides
        resolvedComponents = lib.map
          (c:
            let resolvedName = resolveComponentName c;
            in cfg.toolchain.${c} or cfg.toolchain.${resolvedName} or toolchainComponents.${resolvedName}
          )
          cfg.components;

        # Create aggregated profile with user overrides
        # TODO: this is private API. We're doing this to retain API compatibility with the previous fenix implementation.
        # TODO: the final toolchain derivation/package should be overridable
        # TODO: profiles should be exposed as an option. 99% of uses should be covered by the pre-built profiles with overrides.
        profile = mkAggregated {
          pname = "rust-${cfg.channel}-${toolchain._manifest.version}";
          inherit (toolchain._manifest) version date;
          selectedComponents = resolvedComponents;
        };
      in
      {
        languages.rust.toolchain = builtins.mapAttrs (_: lib.mkDefault) toolchainComponents;
        packages = [ profile ];
      }
    ))
  ]);
}
