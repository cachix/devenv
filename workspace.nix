# Tooling to build the workspace crates
{ lib, callPackage, cargoProfile ? "release" }:

let
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./.cargo
      ./Cargo.toml
      ./Cargo.lock
      ./devenv
      ./devenv-generate
      ./devenv-eval-cache
      ./devenv-cache-core
      ./devenv-run-tests
      ./devenv-tasks
      ./http-client-tls
      ./nix-conf-parser
      ./xtask
    ];
  };

  cargoToml = builtins.fromTOML (builtins.readFile "${src}/Cargo.toml");
  version = cargoToml.workspace.package.version;

  cargoLock = {
    lockFile = "${src}/Cargo.lock";
    outputHashes = {
      "nix-compat-0.1.0" = "sha256-ito4pvET2NEZpiVgEF95HH6VJewQ7p3mJLzPT86o4EA=";
      "wu-manber-0.1.0" = "sha256-7YIttaQLfFC/32utojh2DyOHVsZiw8ul/z0lvOhAE/4=";
    };
  };
in
{
  devenv = callPackage ./devenv/package.nix { inherit src version cargoLock cargoProfile; };
  devenv-tasks = callPackage ./devenv-tasks/package.nix { inherit src version cargoLock cargoProfile; };
}
