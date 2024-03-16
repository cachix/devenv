{ pkgs, inputs }:

pkgs.rustPlatform.buildRustPackage {
  pname = "devenv";
  version = "1.0.0";

  src = pkgs.lib.sourceByRegex ./. [
    "Cargo.toml"
    "Cargo.lock"
    "devenv(/\.*)?"
    "devenv-run-tests(/\.*)?"
  ];

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [ pkgs.makeWrapper ];

  buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
    pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
  ];

  postInstall = ''
    wrapProgram $out/bin/devenv --set DEVENV_NIX ${inputs.nix.packages.${pkgs.stdenv.system}.nix}
  '';
}
