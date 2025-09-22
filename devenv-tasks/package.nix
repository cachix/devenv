{ src
, version
, cargoLock
, cargoProfile ? "release"

, lib
, rustPlatform
}:

rustPlatform.buildRustPackage {
  pname = "devenv-tasks";
  inherit src version cargoLock;

  cargoBuildFlags = [ "-p devenv-tasks" ];
  buildType = cargoProfile;
  doCheck = false;
}
