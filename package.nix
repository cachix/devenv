{ pkgs, inputs }:

pkgs.rustPlatform.buildRustPackage {
  pname = "devenv";
  version = "1.0.3";

  src = pkgs.lib.sourceByRegex ./. [
    "Cargo.toml"
    "Cargo.lock"
    "devenv(/\.*)?"
    "devenv-run-tests(/\.*)?"
  ];

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [ pkgs.makeWrapper pkgs.pkg-config ];

  buildInputs = [ pkgs.openssl ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
    pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
  ];

  postInstall = ''
    wrapProgram $out/bin/devenv --set DEVENV_NIX ${inputs.nix.packages.${pkgs.stdenv.system}.nix} --prefix PATH ":" "$out/bin:${inputs.cachix.packages.${pkgs.stdenv.system}.cachix}/bin"
  '';
}
