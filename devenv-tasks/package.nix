{ src
, version
, cargoLock
, cargoProfile ? "release"
, doCheck ? false # Tests are run by devenv anyways

, rustPlatform
}:

rustPlatform.buildRustPackage {
  pname = "devenv-tasks";
  inherit src version cargoLock;

  env.RUSTFLAGS = "--cfg tracing_unstable";

  cargoBuildFlags = [ "-p devenv-tasks" ];
  buildType = cargoProfile;

  nativeBuildInputs = [
    rustPlatform.bindgenHook
  ];

  inherit doCheck;
}
