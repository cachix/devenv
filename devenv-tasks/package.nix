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
  # Skip tests by default to speed up builds.
  # This is important for builds triggered by the tasks integration.
  doCheck = false;
}
