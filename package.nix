{ lib
, stdenv
, makeBinaryWrapper
, installShellFiles
, rustPlatform
, nix
, cachix ? null
, openssl
, apple-sdk_11
, protobuf
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
    outputHashes = {
      "nix-compat-0.1.0" = "sha256-ito4pvET2NEZpiVgEF95HH6VJewQ7p3mJLzPT86o4EA=";
      "wu-manber-0.1.0" = "sha256-7YIttaQLfFC/32utojh2DyOHVsZiw8ul/z0lvOhAE/4=";
    };
  };

  nativeBuildInputs = [
    installShellFiles
    makeBinaryWrapper
    pkg-config
    protobuf
  ];

  buildInputs = [ openssl ]
    ++ lib.optional stdenv.isDarwin apple-sdk_11;

  # Fix proto files for snix dependencies
  preBuild = ''
    export PROTO_ROOT="$NIX_BUILD_TOP/cargo-vendor-dir"
  '';

  postConfigure = ''
    # Create proto directory structure that snix expects
    cd "$NIX_BUILD_TOP/cargo-vendor-dir"
    mkdir -p snix/{castore,store,build}/protos
    
    # Link proto files to the expected locations
    [ -d snix-castore-*/protos ] && cp snix-castore-*/protos/*.proto snix/castore/protos/ 2>/dev/null || true
    [ -d snix-store-*/protos ] && cp snix-store-*/protos/*.proto snix/store/protos/ 2>/dev/null || true  
    [ -d snix-build-*/protos ] && cp snix-build-*/protos/*.proto snix/build/protos/ 2>/dev/null || true
    
    cd - > /dev/null
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
