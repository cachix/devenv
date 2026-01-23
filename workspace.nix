# Tooling to build the workspace crates
{ lib
, callPackage
, cargoProfile ? "release"
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
          ./devenv-eval-cache
          ./devenv-cache-core
          ./devenv-core
          ./devenv-snix-backend
          ./devenv-nix-backend
          ./devenv-nix-backend-macros
          ./devenv-run-tests
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
      "iocraft-0.7.16" = "sha256-MBwTP8HeJnXnnJqsKkrKIuSk2wxFChotwO58/1JB1js=";
      "nix-compat-0.1.0" = "sha256-dSkomGSFJgTtsxHWsBG8Qy2hqQDuemqDsKRJxvmuZ54=";
      "nix-bindings-bindgen-raw-0.1.0" = "sha256-S/oq8WqYJCyqQAJKgT4n4+2AXGt6cX4wjquQQT8x3Mw=";
      "secretspec-0.6.1" = "sha256-gOmxzGTbKWVXkv2ZPmxxGUV1LB7vOYd7BXqaVd2LaFc=";
      "ser_nix-0.1.2" = "sha256-E1vPfhVDkeSt6OxYhnj8gYadUpJJDLRF5YiUkujQsCQ=";
      "wu-manber-0.1.0" = "sha256-7YIttaQLfFC/32utojh2DyOHVsZiw8ul/z0lvOhAE/4=";
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
