{ pkgs, config, lib, ... }:

let
  cfg = config.languages.rust;

  fenix = config.lib.getInput {
    name = "fenix";
    url = "github:nix-community/fenix";
    attribute = "languages.rust.version";
    follows = [ "nixpkgs" ];
  };
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "languages" "rust" "version" ] [ "languages" "rust" "channel" ])
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
        List of extra [targets](https://github.com/nix-community/fenix#supported-platforms-and-targets)
        to install. Defaults to only the native target.
      '';
    };

    channel = lib.mkOption {
      type = lib.types.enum [ "nixpkgs" "stable" "beta" "nightly" ];
      default = "nixpkgs";
      defaultText = lib.literalExpression ''"nixpkgs"'';
      description = "The rustup toolchain to install.";
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
        rustPackages = fenix.packages.${pkgs.stdenv.system};
        fenixChannel =
          if cfg.channel == "nightly"
          then "latest"
          else cfg.channel;
        toolchain = rustPackages.${fenixChannel};
      in
      {
        languages.rust.toolchain =
          (builtins.mapAttrs (_: pkgs.lib.mkDefault) toolchain);

        packages = [
          (rustPackages.combine (
            (map (c: toolchain.${c}) cfg.components) ++
            (map (t: rustPackages.targets.${t}.${fenixChannel}.rust-std) cfg.targets)
          ))
        ];
      }
    ))
  ]);
}
