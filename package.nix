{ pkgs, inputs }:

pkgs.rustPlatform.buildRustPackage {
  pname = "devenv";
  version = "1.1";

  src = pkgs.lib.sourceByRegex ./. [
    "^\.cargo(/.*)?$"
    "Cargo\.toml"
    "Cargo\.lock"
    "devenv(/.*)?"
    "devenv-run-tests(/.*)?"
    "xtask(/.*)?"
  ];

  cargoBuildFlags = [ "-p devenv -p devenv-run-tests" ];

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [
    pkgs.makeWrapper
    pkgs.pkg-config
    pkgs.installShellFiles
  ];

  buildInputs = [ pkgs.openssl ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
    pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
  ];

  postInstall = ''
    wrapProgram $out/bin/devenv \
      --set DEVENV_NIX ${inputs.nix.packages.${pkgs.stdenv.system}.nix} \
      --prefix PATH ":" "$out/bin:${inputs.cachix.packages.${pkgs.stdenv.system}.cachix}/bin"

    # TODO: problematic for our library...
    wrapProgram $out/bin/devenv-run-tests \
      --set DEVENV_NIX ${inputs.nix.packages.${pkgs.stdenv.system}.nix} \
      --prefix PATH ":" "$out/bin:${inputs.cachix.packages.${pkgs.stdenv.system}.cachix}/bin"

    # Generate manpages
    cargo xtask generate-manpages --out-dir man
    installManPage man/*

    # Generate shell completions
    compdir=./completions
    for shell in bash fish zsh; do
      cargo xtask generate-shell-completion $shell --out-dir $compdir
    done

    installShellCompletion --cmd devenv \
      --bash $compdir/devenv.bash \
      --fish $compdir/devenv.fish \
      --zsh $compdir/_devenv
  '';
}
