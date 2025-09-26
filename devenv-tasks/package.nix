{ src
, version
, cargoLock
, cargoProfile ? "release"
, doCheck ? true

, lib
, rustPlatform
}:

rustPlatform.buildRustPackage {
  pname = "devenv-tasks";
  inherit src version cargoLock;

  cargoBuildFlags = [ "-p devenv-tasks" ];
  buildType = cargoProfile;

  inherit doCheck;
}
