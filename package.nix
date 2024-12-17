{ pkgs, inputs, build_tasks ? false }:

pkgs.rustPlatform.buildRustPackage {
  pname = "devenv";
  version = "1.3.1";

  # WARN: building this from src/modules/tasks.nix fails.
  # There is something being prepended to the path, hence the .*.
  src = pkgs.lib.sourceByRegex ./. [
    ".*\.cargo(/.*)?$"
    ".*Cargo\.toml"
    ".*Cargo\.lock"
    ".*devenv(/.*)?"
    ".*devenv-eval-cache(/.*)?"
    ".*devenv-run-tests(/.*)?"
    ".*devenv-tasks(/.*)?"
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
    pkgs.installShellFiles
    pkgs.makeBinaryWrapper
    pkgs.pkg-config
  ] ++ pkgs.lib.optional (!build_tasks) pkgs.sqlx-cli;

  buildInputs = [ pkgs.openssl ]
    ++ pkgs.lib.optional pkgs.stdenv.isDarwin pkgs.darwin.apple_sdk.frameworks.SystemConfiguration;

  # Force sqlx to use the prepared queries
  SQLX_OFFLINE = true;
  # A local database to use for preparing queries
  DATABASE_URL = "sqlite:nix-eval-cache.db";

  preBuild = pkgs.lib.optionalString (!build_tasks) ''
    cargo sqlx database setup --source devenv-eval-cache/migrations
    cargo sqlx prepare --workspace
  '';

  postInstall =
    let
      inherit (inputs.nix.packages.${pkgs.stdenv.system}) nix;
      inherit (inputs.cachix.packages.${pkgs.stdenv.system}) cachix;

      localeArchiveFix =
        pkgs.lib.optionalString (pkgs.stdenv.isLinux && (pkgs.glibcLocalesUtf8 != null)) ''
          --set-default LOCALE_ARCHIVE ${pkgs.glibcLocalesUtf8}/lib/locale/locale-archive
        '';
    in
    pkgs.lib.optionalString (!build_tasks) ''
      wrapProgram $out/bin/devenv \
        --prefix PATH ":" "$out/bin:${cachix}/bin" \
        --set DEVENV_NIX ${nix} \
        ${localeArchiveFix} \

      # TODO: problematic for our library...
      wrapProgram $out/bin/devenv-run-tests \
        --prefix PATH ":" "$out/bin:${cachix}/bin" \
        --set DEVENV_NIX ${nix} \
        ${localeArchiveFix} \

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
