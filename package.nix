{ lib
, stdenv
, makeBinaryWrapper
, installShellFiles
, rustPlatform
, nix
, cachix ? null
, openssl
, apple-sdk_11
, pkg-config
, glibcLocalesUtf8
, build_tasks ? false
}:

rustPlatform.buildRustPackage {
  pname = "devenv${lib.optionalString build_tasks "-tasks"}";
  version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;

  # WARN: building this from src/modules/tasks.nix fails.
  # There is something being prepended to the path, hence the .*.
  src = lib.sourceByRegex ./. [
    ".*\.cargo(/.*)?$"
    ".*Cargo\.toml"
    ".*Cargo\.lock"
    ".*devenv(/.*)?"
    ".*devenv-generate(/.*)?"
    ".*devenv-eval-cache(/.*)?"
    ".*devenv-cache-core(/.*)?"
    ".*devenv-run-tests(/.*)?"
    ".*devenv-tasks(/.*)?"
    ".*http-client-tls(/.*)?"
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
  ];

  buildInputs = [ openssl ]
    ++ lib.optional stdenv.isDarwin apple-sdk_11;

  postInstall =
    let
      setDefaultLocaleArchive =
        lib.optionalString (glibcLocalesUtf8 != null) ''
          --set-default LOCALE_ARCHIVE ${glibcLocalesUtf8}/lib/locale/locale-archive
        '';
    in
    lib.optionalString (!build_tasks) ''
      wrapProgram $out/bin/devenv \
        --prefix PATH ":" "$out/bin:${lib.getBin cachix}/bin" \
        --set DEVENV_NIX ${nix} \
        ${setDefaultLocaleArchive} \

      # TODO: problematic for our library...
      wrapProgram $out/bin/devenv-run-tests \
        --prefix PATH ":" "$out/bin:${lib.getBin cachix}/bin" \
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
