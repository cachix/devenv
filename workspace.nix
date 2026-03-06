# Tooling to build the workspace crates

{
  # The profile to build with, e.g. "release" or "debug".
  cargoProfile ? "release"
, # The git revision to display in `devenv version`.
  gitRev ? ""
, # Whether this is a release. If false, release update checks are disabled.
  isRelease ? false

, # The Nix package to use.
  nix

, lib
, callPackage
,
}:

let
  src = lib.fileset.toSource {
    root = ./.;
    fileset =
      lib.fileset.difference
        (lib.fileset.unions [
          ./.cargo
          ./Cargo.toml
          ./Cargo.lock
          ./devenv
          ./devenv-activity
          ./devenv-activity-macros
          ./devenv-cache-core
          ./devenv-core
          ./devenv-eval-cache
          ./devenv-event-sources
          ./devenv-nix-backend
          ./devenv-nix-backend-macros
          ./devenv-processes
          ./devenv-reload
          ./devenv-run-tests
          ./devenv-shell
          ./devenv-snix-backend
          ./devenv-tasks
          ./devenv-tui
          ./nix-conf-parser
          ./tokio-shutdown
          ./xtask
        ])
        # Ignore local builds
        (lib.fileset.fileFilter (file: file.name == "target") ./.);
  };

  cargoToml = builtins.fromTOML (builtins.readFile "${src}/Cargo.toml");
  version = cargoToml.workspace.package.version;

  cargoLock = {
    lockFile = "${src}/Cargo.lock";
    outputHashes = {
      "iocraft-0.7.18" = "sha256-kgbJ6bE1shp+jBz1lN1SnG2D5UbwCT3+K/nRMWVun/E=";
      "crossterm-0.28.1" = "sha256-EC3HTF/l9E+3DnsLfB6L+SDNmfgWWJOSq8Oo+rQ3dVQ=";
      "nix-compat-0.1.0" = "sha256-dSkomGSFJgTtsxHWsBG8Qy2hqQDuemqDsKRJxvmuZ54=";
      "nix-bindings-bindgen-raw-0.1.0" = "sha256-t+sEdTZysfKROjFxpsdjp9tN8yNqz+f0+lYOMkkjIxA=";
      "wu-manber-0.1.0" = "sha256-7YIttaQLfFC/32utojh2DyOHVsZiw8ul/z0lvOhAE/4=";
      "secretspec-0.7.2" = "sha256-vX4hbA1v7AsnNf+PoSzkKeMMkGlmR9GXAg75ggoSeVE=";

    };
  };
in
{
  inherit version;

  crates = {
    devenv = callPackage ./devenv/package.nix {
      inherit
        src
        version
        cargoLock
        cargoProfile
        gitRev
        isRelease
        nix
        ;
    };

    devenv-tasks = callPackage ./devenv-tasks/package.nix {
      inherit
        src
        version
        cargoLock
        cargoProfile
        ;
    };

    # A custom tasks build for the module system.
    # Use a faster release profile and skip tests.
    devenv-tasks-fast-build = callPackage ./devenv-tasks/package.nix {
      inherit src version cargoLock;
      cargoProfile = "release_fast";
      doCheck = false;
    };
  };
}
