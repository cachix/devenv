{ src
, version
, cargoLock
, cargoProfile ? "release"

, lib
, stdenv
, makeBinaryWrapper
, installShellFiles
, rustPlatform
, devenv-nix
, cachix
, openssl
, dbus
, protobuf
, pkg-config
, glibcLocalesUtf8
}:

rustPlatform.buildRustPackage {
  pname = "devenv";
  inherit src version cargoLock;

  cargoBuildFlags = [ "-p devenv -p devenv-run-tests" ];

  nativeBuildInputs = [
    installShellFiles
    makeBinaryWrapper
    pkg-config
    protobuf
  ];

  buildInputs = [
    openssl
  ]
  # secretspec
  ++ lib.optional (stdenv.isLinux) dbus;

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

  # Fix proto files for snix dependencies
  preBuild = ''
    export PROTO_ROOT="$NIX_BUILD_TOP/cargo-vendor-dir"
  '';

  postInstall =
    let
      setDefaultLocaleArchive =
        lib.optionalString (glibcLocalesUtf8 != null) ''
          --set-default LOCALE_ARCHIVE ${glibcLocalesUtf8}/lib/locale/locale-archive
        '';
    in
    ''
      wrapProgram $out/bin/devenv \
        --prefix PATH ":" "$out/bin:${lib.getBin cachix}/bin" \
        --set DEVENV_NIX ${devenv-nix} \
        ${setDefaultLocaleArchive} \

      # TODO: problematic for our library...
      wrapProgram $out/bin/devenv-run-tests \
        --prefix PATH ":" "$out/bin:${lib.getBin cachix}/bin" \
        --set DEVENV_NIX ${devenv-nix} \
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
