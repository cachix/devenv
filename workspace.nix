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
          ./devenv-generate
          ./devenv-eval-cache
          ./devenv-cache-core
          ./devenv-core
          ./devenv-snix-backend
          ./devenv-nix-backend
          ./devenv-run-tests
          ./devenv-tasks
          ./devenv-tui
          ./http-client-tls
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
      "nix-compat-0.1.0" = "sha256-dSkomGSFJgTtsxHWsBG8Qy2hqQDuemqDsKRJxvmuZ54=";
      "nix-bindings-bindgen-raw-0.1.0" = "sha256-sgeWahdzWd0Gmhlew0wNxH11T6b8sHNlYhQg1Hrzqys=";
      "secretspec-0.5.0" = "sha256-YKBZcdbR62IxchnGO/Vn5hWac3phvAlE6gGeAhBS50A=";
      "ser_nix-0.1.2" = "sha256-IjTsHTAEBQQ8xyDHW51wufu2mmfmiw+alVjrLrG8bkY=";
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
