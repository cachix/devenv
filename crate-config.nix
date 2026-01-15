# Crate overrides for crate2nix build
{ lib
, stdenv
, nix
, openssl
, dbus
, protobuf
, pkg-config
, llvmPackages
, boehmgc
, cachix
, nixd
, makeBinaryWrapper
, installShellFiles
, glibcLocalesUtf8
, rustPlatform
, gitRev ? ""
}:

let
  nixLibs = [
    nix.libs.nix-expr-c
    nix.libs.nix-store-c
    nix.libs.nix-util-c
    nix.libs.nix-flake-c
    nix.libs.nix-cmd-c
    nix.libs.nix-fetchers-c
    nix.libs.nix-main-c
    boehmgc
    llvmPackages.clang-unwrapped
  ];

  protoSetup = ''
    # Create proto directory structure that snix expects
    if [ -d "$NIX_BUILD_TOP/cargo-vendor-dir" ]; then
      pushd "$NIX_BUILD_TOP/cargo-vendor-dir"
      mkdir -p snix/{castore,store,build}/protos

      # Link proto files to the expected locations
      [ -d snix-castore-*/protos ] && cp snix-castore-*/protos/*.proto snix/castore/protos/ 2>/dev/null || true
      [ -d snix-store-*/protos ] && cp snix-store-*/protos/*.proto snix/store/protos/ 2>/dev/null || true
      [ -d snix-build-*/protos ] && cp snix-build-*/protos/*.proto snix/build/protos/ 2>/dev/null || true

      popd
    fi
    export PROTO_ROOT="$NIX_BUILD_TOP/cargo-vendor-dir"
  '';

  # Common overrides for crates needing openssl
  opensslOverride = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ [ openssl ];
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
  };

  # Override for crates needing protobuf
  protobufOverride = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ protobuf ];
    preConfigure = (attrs.preConfigure or "") + protoSetup;
  };

  # Override for crates needing dbus (Linux only)
  dbusOverride = attrs: {
    buildInputs = (attrs.buildInputs or [ ])
      ++ lib.optional stdenv.isLinux dbus;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
  };
in
{
  # Main devenv crate - needs all the things
  devenv = attrs: {
    buildInputs = (attrs.buildInputs or [ ])
      ++ [ openssl ]
      ++ nixLibs
      ++ lib.optional stdenv.isLinux dbus;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [
      pkg-config
      protobuf
      rustPlatform.bindgenHook
      makeBinaryWrapper
      installShellFiles
    ];
    preConfigure = (attrs.preConfigure or "") + protoSetup;
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
    DEVENV_GIT_REV = gitRev;
  };

  # devenv-run-tests needs the same deps as devenv
  devenv-run-tests = attrs: {
    buildInputs = (attrs.buildInputs or [ ])
      ++ [ openssl ]
      ++ nixLibs
      ++ lib.optional stdenv.isLinux dbus;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [
      pkg-config
      protobuf
      rustPlatform.bindgenHook
    ];
    preConfigure = (attrs.preConfigure or "") + protoSetup;
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
  };

  # devenv-nix-backend needs nix libs
  devenv-nix-backend = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ nixLibs;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [
      pkg-config
      rustPlatform.bindgenHook
    ];
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
  };

  # devenv-snix-backend needs protobuf
  devenv-snix-backend = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ protobuf ];
    preConfigure = (attrs.preConfigure or "") + protoSetup;
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
  };

  # snix crates need protobuf
  snix-castore = protobufOverride;
  snix-store = protobufOverride;
  snix-build = protobufOverride;
  snix-glue = protobufOverride;

  # secretspec needs dbus on Linux
  secretspec = dbusOverride;

  # openssl-sys needs openssl
  openssl-sys = opensslOverride;

  # Other crates that need tracing_unstable
  devenv-core = attrs: {
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
  };

  devenv-eval-cache = attrs: {
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
  };

  devenv-tasks = attrs: {
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
  };

  devenv-tui = attrs: {
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
  };

  devenv-activity = attrs: {
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
  };

  # The tracing crate needs tracing_unstable to enable valuable support
  tracing = attrs: {
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
  };

  tracing-core = attrs: {
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
  };

  # rmcp uses env!("CARGO_CRATE_NAME") at compile time
  rmcp = attrs: {
    CARGO_CRATE_NAME = "rmcp";
  };

  # nix-bindings crates need pkg-config, nix libs, and bindgen
  nix-bindings-bindgen-raw = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ nixLibs;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [
      pkg-config
      rustPlatform.bindgenHook
    ];
  };

  nix-bindings-util = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ nixLibs;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
  };

  nix-bindings-store = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ nixLibs;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
  };

  nix-bindings-expr = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ nixLibs;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
  };

  nix-bindings-flake = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ nixLibs;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
  };

  nix-bindings-fetchers = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ nixLibs;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
  };

  nix-cmd = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ nixLibs;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
  };
}
