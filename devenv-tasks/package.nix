{ src
, version
, cargoLock
, cargoProfile ? "release"
, doCheck ? false # Tests are run by devenv anyways

, lib
, rustPlatform
}:

rustPlatform.buildRustPackage {
  pname = "devenv-tasks";
  inherit src version cargoLock;

  cargoBuildFlags = [ "-p devenv-tasks" ];
  buildType = cargoProfile;
}
