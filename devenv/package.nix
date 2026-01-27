{ src
, version
, cargoLock
, cargoProfile ? "release"
, gitRev ? ""
, isRelease ? false
, lib
, stdenv
, makeBinaryWrapper
, installShellFiles
, rustPlatform
, cachix
, nixd
, gitMinimal
, openssl
, dbus
, protobuf
, pkg-config
, glibcLocalesUtf8
, nix
, llvmPackages
, boehmgc
}:

rustPlatform.buildRustPackage {
  pname = "devenv";
  inherit src version cargoLock;

  RUSTFLAGS = "--cfg tracing_unstable";
  DEVENV_GIT_REV = gitRev;
  DEVENV_IS_RELEASE = if isRelease then "1" else "";
  VERGEN_IDEMPOTENT = "1";

  cargoBuildFlags = [ "-p devenv -p devenv-run-tests" ];
  buildType = cargoProfile;

  nativeBuildInputs = [
    installShellFiles
    makeBinaryWrapper
    pkg-config
    protobuf
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    openssl
    nix.libs.nix-expr-c
    nix.libs.nix-store-c
    nix.libs.nix-util-c
    nix.libs.nix-flake-c
    nix.libs.nix-cmd-c
    nix.libs.nix-fetchers-c
    nix.libs.nix-main-c
    boehmgc
    llvmPackages.clang-unwrapped
  ]
  # secretspec
  ++ lib.optional stdenv.isLinux dbus;

  postConfigure = ''
    # Create proto directory structure that snix expects
    pushd "$NIX_BUILD_TOP/cargo-vendor-dir"
    mkdir -p snix/{castore,store,build}/protos

    # Link proto files to the expected locations
    [ -d snix-castore-*/protos ] && cp snix-castore-*/protos/*.proto snix/castore/protos/ 2>/dev/null || true
    [ -d snix-store-*/protos ] && cp snix-store-*/protos/*.proto snix/store/protos/ 2>/dev/null || true
    [ -d snix-build-*/protos ] && cp snix-build-*/protos/*.proto snix/build/protos/ 2>/dev/null || true

    popd
  '';

  preBuild = ''
    # Fix proto files for snix dependencies
    export PROTO_ROOT="$NIX_BUILD_TOP/cargo-vendor-dir"
  '';

  nativeCheckInputs = [ gitMinimal ];
  preCheck = ''
    # Initialize git repo for tests that use git-root-relative imports
    pushd $NIX_BUILD_TOP/source
    git init -b main
    git add -A
    popd
  '';

  # Skip devenv-nix-backend tests in sandbox due to store permission restrictions
  cargoTestFlags = [ "--workspace" "--exclude" "devenv-nix-backend" ];

  postInstall =
    let
      setDefaultLocaleArchive = lib.optionalString (glibcLocalesUtf8 != null) ''
        --set-default LOCALE_ARCHIVE ${glibcLocalesUtf8}/lib/locale/locale-archive
      '';
    in
    ''
      wrapProgram $out/bin/devenv \
        --prefix PATH ":" "$out/bin:${lib.getBin cachix}/bin:${lib.getBin nixd}/bin" \
        ${setDefaultLocaleArchive} \

      # TODO: problematic for our library...
      wrapProgram $out/bin/devenv-run-tests \
        --prefix PATH ":" "$out/bin:${lib.getBin cachix}/bin:${lib.getBin nixd}/bin" \
        ${setDefaultLocaleArchive} \

      # Generate manpages
      cargo xtask generate-manpages --out-dir man
      installManPage man/*

      # Generate shell completions (devenv must be in PATH)
      compdir=./completions
      export PATH="$out/bin:$PATH"
      for shell in bash fish zsh; do
        cargo xtask generate-shell-completion $shell --out-dir $compdir
      done

      installShellCompletion --cmd devenv \
        --bash $compdir/devenv.bash \
        --fish $compdir/devenv.fish \
        --zsh $compdir/_devenv
    '';
}
