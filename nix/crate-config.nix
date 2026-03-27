# Crate overrides for crate2nix build
{ lib
, stdenv
, nix
, openssl
, dbus
, protobuf
, pkg-config
, llvmPackages
, rustPlatform
, libghostty-vt
, gitRev ? ""
, isRelease ? false
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

  tracingUnstable = attrs: {
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "--cfg" "tracing_unstable" ];
  };

  # Override for crates needing nix C libraries
  nixLibsOverride = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ nixLibs;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
  };

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

  # Shared override for crates linking against nix, openssl, protobuf, dbus, and bindgen
  devenvBase = attrs: {
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
in
{
  # Main devenv crate
  devenv = attrs: devenvBase attrs // {
    DEVENV_GIT_REV = gitRev;
    DEVENV_IS_RELEASE = if isRelease then "true" else "";
  };

  # devenv-run-tests needs the same deps as devenv
  devenv-run-tests = devenvBase;

  # xtask links the devenv crate, so it needs the same native libs
  xtask = devenvBase;

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

  # Crates that need tracing_unstable
  devenv-core = tracingUnstable;
  devenv-eval-cache = tracingUnstable;
  devenv-tasks = tracingUnstable;
  devenv-tui = tracingUnstable;
  devenv-activity = tracingUnstable;

  # The tracing crate needs tracing_unstable to enable valuable support
  tracing = tracingUnstable;
  tracing-core = tracingUnstable;

  # netstat2 uses bindgen in its build script, needs libclang
  netstat2 = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [
      rustPlatform.bindgenHook
    ];
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

  # libghostty-vt-sys builds ghostty VT from source using zig + git, which
  # fails in the Nix sandbox.  Replace the build script with a stub that
  # links against the pre-built library instead.
  libghostty-vt-sys = attrs: {
    preConfigure = (attrs.preConfigure or "") + ''
      cat > build.rs << 'BUILDRS'
      fn main() {
          println!("cargo:rustc-link-search=native=${libghostty-vt}/lib");
          println!("cargo:rustc-link-lib=dylib=ghostty-vt");
          println!("cargo:include=${libghostty-vt}/include");
      }
      BUILDRS
    '';
  };

  nix-bindings-util = nixLibsOverride;
  nix-bindings-store = nixLibsOverride;
  nix-bindings-expr = nixLibsOverride;
  nix-bindings-flake = nixLibsOverride;
  nix-bindings-fetchers = nixLibsOverride;
  nix-cmd = nixLibsOverride;
}
