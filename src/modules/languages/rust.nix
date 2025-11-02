{ pkgs, config, lib, ... }:

let
  cfg = config.languages.rust;

  validChannels = [ "nixpkgs" "stable" "beta" "nightly" ];

  rust-overlay = config.lib.getInput {
    name = "rust-overlay";
    url = "github:oxalica/rust-overlay";
    attribute = "languages.rust.channel";
    follows = [ "nixpkgs" ];
  };

  crate2nix = config.lib.getInput {
    name = "crate2nix";
    url = "github:nix-community/crate2nix";
    attribute = "languages.rust.import";
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

    toolchainFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        Path to a `rust-toolchain` or `rust-toolchain.toml` file for automatic toolchain configuration.

        When set, devenv will use rust-overlay's `fromRustupToolchainFile` to automatically
        configure the toolchain based on the file contents (channel, components, targets, profile).

        This follows the standard Rust toolchain file format documented at:
        https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file

        Cannot be used together with manual `channel` or `version` configuration.

        Example:
        ```nix
        languages.rust.toolchainFile = ./rust-toolchain.toml;
        ```
      '';
      example = lib.literalExpression "./rust-toolchain.toml";
    };

    toolchainPackage = lib.mkOption {
      type = lib.types.package;
      description = ''
        The aggregated toolchain package, which includes the configured components and targets.
        This is automatically set based on the channel and components configuration.
      '';
    };

    import = lib.mkOption {
      type = lib.types.functionTo (lib.types.functionTo lib.types.package);
      description = ''
        Import a Cargo project using cargo2nix.

        This function takes a path to a directory containing a Cargo.toml file
        and returns a derivation that builds the Rust project using cargo2nix.

        Example usage:
        ```nix
        let
        mypackage = config.languages.rust.import ./path/to/cargo/project {};
        in {
        languages.rust.enable = true;
        packages = [ mypackage ];
        }
        ```
      '';
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      languages.rust.import = path: args:
        let
          crate2nixTools = pkgs.callPackage "${crate2nix}/tools.nix" { };

          # Try to infer package name from Cargo.toml or use directory name as fallback
          packageName = args.packageName or (
            let
              cargoToml =
                if builtins.pathExists (path + "/Cargo.toml")
                then builtins.fromTOML (builtins.readFile (path + "/Cargo.toml"))
                else { };
            in
              cargoToml.package.name or (builtins.baseNameOf (builtins.toString path))
          );

          # Use crate2nix IFD to auto-generate
          cargoNix = crate2nixTools.appliedCargoNix {
            name = packageName;
            src = path;
          };
        in
        cargoNix.rootCrate.build.override args;
    }
    (
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
          {
            assertion = cfg.toolchainFile == null || (cfg.channel == "nixpkgs" && cfg.version == "latest");
            message = ''
              Cannot use `languages.rust.toolchainFile` together with manual channel or version configuration.

              When using `toolchainFile`, the toolchain configuration (channel, version, components, targets)
              is automatically read from the rust-toolchain file.

              Either:
              - Remove the `toolchainFile` option and configure manually, or
              - Keep `toolchainFile` and remove manual `channel` and `version` settings
            '';
          }
          {
            assertion = cfg.toolchainFile == null || cfg.targets == [ ];
            message = ''
              Cannot use `languages.rust.toolchainFile` with manual `targets` configuration.

              When using `toolchainFile`, targets are automatically read from the rust-toolchain file.
              Remove the `targets` option or configure targets in your rust-toolchain.toml instead.
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
          };

        git-hooks.tools =
          let
            mkOverrideTool = lib.mkOverride (lib.modules.defaultOverridePriority - 1);
          in
          {
            cargo = mkOverrideTool cfg.toolchainPackage;
            rustfmt = mkOverrideTool cfg.toolchainPackage;
            clippy = mkOverrideTool cfg.toolchainPackage;
          };

        # Allow clippy to access the internet to fetch dependencies.
        git-hooks.hooks.clippy.settings.offline = lib.mkDefault false;
      }
    )

    (lib.mkIf (cfg.toolchainFile != null) (
      let
        rustBin = rust-overlay.lib.mkRustBin { } pkgs.buildPackages;
        toolchainFromFile = rustBin.fromRustupToolchainFile cfg.toolchainFile;
      in
      {
        languages.rust.toolchainPackage = toolchainFromFile;
        packages = [ cfg.toolchainPackage ];
      }
    ))

    (lib.mkIf (cfg.toolchainFile == null && cfg.channel == "nixpkgs") {
      languages.rust.toolchainPackage = lib.mkDefault (
        pkgs.symlinkJoin {
          name = "rust-toolchain-${cfg.channel}";
          paths = builtins.map (c: cfg.toolchain.${c} or (throw "toolchain.${c}")) cfg.components;
        }
      );
      packages = [ cfg.toolchainPackage ];
    })

    (lib.mkIf (cfg.toolchainFile == null && cfg.channel != "nixpkgs") (
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
        # Extract individual components from toolchain, avoiding the 'rust' profile, which triggers warnings.
        # This ensures target components like rust-std-${target} are available
        toolchainComponents = builtins.removeAttrs toolchain [ "rust" ];

        # Get available targets from the manifest
        availableTargets = toolchain._manifest.pkg.rust-std.target or { };
        allComponents = toolchain._components or { };
        availableComponents = toolchain._manifest.profiles.complete or [ ];

        # Ensure native platform target is always included for rust-overlay
        # Read the native target from the nixpkgs config.
        nativeTarget = pkgs.stdenv.hostPlatform.rust.rustcTargetSpec;
        allTargets = lib.unique ([ nativeTarget ] ++ cfg.targets);

        targetComponents = lib.map
          (target:
            let
              targetComponentSet = allComponents.${target} or { };
              targetRustStd = targetComponentSet.rust-std or null;
            in
            if !(availableTargets ? ${target})
            then throw "Target '${target}' not available in manifest. Available targets: ${lib.concatStringsSep ", " (builtins.attrNames availableTargets)}"
            else if targetRustStd == null
            then throw "Target '${target}' component not found in toolchain. Available targets: ${lib.concatStringsSep ", " (builtins.attrNames availableTargets)}"
            else targetRustStd
          )
          allTargets;

        # Resolve regular components with user overrides
        # Try the component name, then with the -preview suffix for rust-overlay compatibility
        resolvedComponents = lib.map
          (c:
            let
              resolvedName =
                if builtins.elem c availableComponents then c
                else if builtins.elem "${c}-preview" availableComponents then "${c}-preview"
                else throw "Component '${c}' not found. Available: ${lib.concatStringsSep ", " availableComponents}";
            in
              cfg.toolchain.${c} or cfg.toolchain.${resolvedName} or toolchainComponents.${resolvedName}
          )
          cfg.components;

        allSelectedComponents = resolvedComponents ++ targetComponents;

        # Create aggregated profile with user overrides and target components
        # NOTE: this uses private API to retain API compatibility with the previous fenix implementation.
        # The final toolchain derivation/package should be overridable and profiles should be exposed as an option.
        # 99% of uses should be covered by the pre-built profiles with overrides.
        profile = mkAggregated {
          pname = "rust-${cfg.channel}-${toolchain._manifest.version}";
          inherit (toolchain._manifest) version date;
          selectedComponents = allSelectedComponents;
        };
      in
      {
        languages.rust.toolchain = builtins.mapAttrs (_: lib.mkDefault) toolchainComponents;
        languages.rust.toolchainPackage = lib.mkDefault profile;
        packages = [ cfg.toolchainPackage ];
      }
    ))
  ]);
}
