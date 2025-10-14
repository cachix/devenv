# Tooling to build the workspace crates
{ lib, callPackage, cargoProfile ? "release" }:

let
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.difference
      (lib.fileset.unions [
        ./.cargo
        ./Cargo.toml
        ./Cargo.lock
        ./devenv
        ./devenv-generate
        ./devenv-eval-cache
        ./devenv-cache-core
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
      "nix-compat-0.1.0" = "sha256-ehkc7ug0HkTHKxx9tQi/9vkOtQry9HdiX9UxBUvjYRY=";
      "wu-manber-0.1.0" = "sha256-7YIttaQLfFC/32utojh2DyOHVsZiw8ul/z0lvOhAE/4=";
    };
  };
in
{
  devenv = callPackage ./devenv/package.nix { inherit src version cargoLock cargoProfile; };

  devenv-tasks = callPackage ./devenv-tasks/package.nix { inherit src version cargoLock cargoProfile; };

  # A custom tasks build for the module system.
  # Use a faster release profile and skip tests.
  devenv-tasks-fast-build = callPackage ./devenv-tasks/package.nix {
    inherit src version cargoLock;
    cargoProfile = "release_fast";
    doCheck = false;
  };
}
