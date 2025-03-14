{ lib
, stdenv
, makeBinaryWrapper
, installShellFiles
, rustPlatform
, nix
, cachix
, darwin
, sqlx-cli
, openssl
, pkg-config
, glibcLocalesUtf8
, build_tasks ? false
}:

rustPlatform.buildRustPackage {
  pname = "devenv";
  version = "1.4.2";

  # WARN: building this from src/modules/tasks.nix fails.
  # There is something being prepended to the path, hence the .*.
  src = lib.sourceByRegex ./. [
    ".*\.cargo(/.*)?$"
    ".*Cargo\.toml"
    ".*Cargo\.lock"
    ".*devenv(/.*)?"
    ".*devenv-generate(/.*)?"
    ".*devenv-eval-cache(/.*)?"
    ".*devenv-run-tests(/.*)?"
    ".*devenv-tasks(/.*)?"
    "direnvrc"
    ".*nix-conf-parser(/.*)?"
    ".*xtask(/.*)?"
  ];

  cargoBuildFlags =
    if build_tasks
    then [ "-p devenv-tasks" ]
    else [ "-p devenv -p devenv-run-tests" ];

  doCheck = !build_tasks;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [
    installShellFiles
    makeBinaryWrapper
    pkg-config
  ] ++ lib.optional (!build_tasks) sqlx-cli;

  buildInputs = [ openssl ]
    ++ lib.optional stdenv.isDarwin darwin.apple_sdk.frameworks.SystemConfiguration;

  # Force sqlx to use the prepared queries
  SQLX_OFFLINE = true;
  # A local database to use for preparing queries
  DATABASE_URL = "sqlite:nix-eval-cache.db";

  preBuild = lib.optionalString (!build_tasks) ''
    cargo sqlx database setup --source devenv-eval-cache/migrations
    cargo sqlx prepare --workspace
  '';

  postInstall =
    let
      setDefaultLocaleArchive =
        lib.optionalString (glibcLocalesUtf8 != null) ''
          --set-default LOCALE_ARCHIVE ${glibcLocalesUtf8}/lib/locale/locale-archive
        '';
    in
    lib.optionalString (!build_tasks) ''
      wrapProgram $out/bin/devenv \
        --prefix PATH ":" "$out/bin:${cachix}/bin" \
        --set DEVENV_NIX ${nix} \
        ${setDefaultLocaleArchive} \

      # TODO: problematic for our library...
      wrapProgram $out/bin/devenv-run-tests \
        --prefix PATH ":" "$out/bin:${cachix}/bin" \
        --set DEVENV_NIX ${nix} \
        ${setDefaultLocaleArchive} \

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
