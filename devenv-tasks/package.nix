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

  RUSTFLAGS = "--cfg tracing_unstable";

  cargoBuildFlags = [ "-p devenv-tasks" ];
  buildType = cargoProfile;

  inherit doCheck;
}
