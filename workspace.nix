# Tooling to build the workspace crates
{ lib
, callPackage
, cargoProfile ? "release"
, gitRev ? ""
, isRelease ? false
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
      "iocraft-0.7.16" = "sha256-iSvX3wzHHkqS0HtjEGQRV7p4LHaGaNrgmK0/iPuuy24=";
      "crossterm-0.28.1" = "sha256-EC3HTF/l9E+3DnsLfB6L+SDNmfgWWJOSq8Oo+rQ3dVQ=";
      "nix-compat-0.1.0" = "sha256-dSkomGSFJgTtsxHWsBG8Qy2hqQDuemqDsKRJxvmuZ54=";
      "nix-bindings-bindgen-raw-0.1.0" = "sha256-XtnN/Moc0OZES37NTmAeLzwkVFSinpZ4qHrMdGOaBdI=";
      "wu-manber-0.1.0" = "sha256-7YIttaQLfFC/32utojh2DyOHVsZiw8ul/z0lvOhAE/4=";
      "inquire-0.9.3" = "sha256-3A3QH9nuAsfxEoxgYCbEitnlV7mQltZX2vJ3Uv3q6Ys=";
      "secretspec-0.7.1" = "sha256-ofwxnnWmyf/qnDN2DNbcltm+PiwW/mFnYhUfdzZViLA=";
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
