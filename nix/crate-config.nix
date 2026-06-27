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
, pcre2
, bzip2
, libunistring
, llhttp
, mimalloc
, gitRev ? ""
, isRelease ? false
,
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
  ]
  # libclang is only needed by bindgen, which runs on the build host and gets
  # it from `rustPlatform.bindgenHook`; nothing links libclang at runtime. On
  # the static (musl) build, pulling clang-unwrapped in as a buildInput forces
  # an enormous static LLVM+clang compile whose final tool links exhaust RAM
  # and disk. Keep it on glibc (cheap, already cached); drop it for the static
  # build and rely on bindgenHook alone.
  ++ lib.optional (!stdenv.hostPlatform.isStatic) llvmPackages.clang-unwrapped
  ++ [
    # [static-link-spike] With static Nix libs, pkg-config (PKG_CONFIG_ALL_STATIC)
    # walks the full Requires.private tree; libgit2 needs libpcre2-8, whose .pc
    # isn't otherwise on the path. Add it so the static link resolves.
    pcre2.dev
    # These transitive static deps (libarchive→bz2, libidn2→unistring, curl→llhttp)
    # have no .pc file, so pkg-config emits `-lbz2`/`-lunistring`/`-lllhttp` with no
    # `-L`; add the packages so their lib dirs reach the linker search path.
    bzip2
    libunistring
    llhttp
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
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [
      "--cfg"
      "tracing_unstable"
    ];
  };

  # [static-link-spike] Tell pkg-config to emit the static link line (Libs.private),
  # so the now-static Nix C++ libs (libnixstore/expr/util) are pulled in transitively
  # when crates link the C-API libs.
  staticPkgConfig = { PKG_CONFIG_ALL_STATIC = "1"; };

  # [static-link-spike] The static Nix archives reference boost's *compiled*
  # component libs (iostreams/context/url), which boost's pkg-config doesn't
  # enumerate. boost's lib dir is already on -L (propagated), so link them
  # explicitly. --start-group handles the archive<->boost reference ordering.
  #
  # The Nix libs are C++ (built with musl g++), so the final binary also needs
  # the C++ runtime for `operator new`/`operator delete`/etc. Rust links libc
  # but not libstdc++, and on the dynamic (glibc) build these come in via the
  # Nix .so's NEEDED; for the static build we must link libstdc++ ourselves.
  # Put it in the same group so it resolves symbols the Nix archives reference.
  staticBoostLinkOpts = [
    "-C"
    "link-arg=-Wl,--start-group"
    "-C"
    "link-arg=-lboost_context"
    "-C"
    "link-arg=-lboost_iostreams"
    "-C"
    "link-arg=-lboost_url"
    "-C"
    "link-arg=-lstdc++"
    # libstdc++ pulls wide-char/locale/threading helpers from libc
    # (btowc, wmemset, setlocale, get_nprocs, …). rustc's own -lc is emitted
    # before this trailing group, so ld can't back-resolve those; include -lc
    # inside the group so --start-group/--end-group iterates until resolved.
    "-C"
    "link-arg=-lc"
    "-C"
    "link-arg=-Wl,--end-group"
  ];

  # Override for crates needing nix C libraries
  nixLibsOverride = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ nixLibs;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
  } // staticPkgConfig;

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
    buildInputs = (attrs.buildInputs or [ ]) ++ lib.optional stdenv.isLinux dbus;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
  };

  # [tier2] On the static (musl) build, use mimalloc instead of musl's mallocng.
  # musl returns freed memory to the kernel aggressively, so devenv's
  # alloc-heavy init triggers hundreds of mmap/munmap syscalls (~8 ms of system
  # time). mimalloc keeps a userspace heap and avoids that churn.
  #
  # We can't override `malloc` at link time (Rust links its bundled musl libc.a
  # before our -lmimalloc → multiple-definition). Instead build mimalloc with
  # MI_OVERRIDE=OFF (exposes only the `mi_*` API, no `malloc` symbols) and route
  # Rust's allocator to it via a `#[global_allocator]` gated on `--cfg
  # use_mimalloc` (see devenv/src/main.rs).
  mimallocNoOverride = mimalloc.overrideAttrs (o: {
    cmakeFlags = (o.cmakeFlags or [ ]) ++ [ "-DMI_OVERRIDE=OFF" ];
  });
  staticAllocLinkOpts = lib.optionals stdenv.hostPlatform.isStatic [
    "--cfg"
    "use_mimalloc"
    "-C"
    "link-arg=-lmimalloc"
  ];

  # Shared override for crates linking against nix, openssl, protobuf, dbus, and bindgen.
  devenvBase = attrs: {
    buildInputs =
      (attrs.buildInputs or [ ])
        ++ [
        openssl
        libghostty-vt
      ]
        ++ nixLibs
        ++ lib.optional stdenv.isLinux dbus
        ++ lib.optional stdenv.hostPlatform.isStatic mimallocNoOverride;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [
      pkg-config
      protobuf
      rustPlatform.bindgenHook
    ];
    preConfigure = (attrs.preConfigure or "") + protoSetup;
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [
      "--cfg"
      "tracing_unstable"
    ] ++ staticAllocLinkOpts ++ staticBoostLinkOpts;
  } // staticPkgConfig;
in
{
  # Main devenv crate
  devenv =
    attrs:
    devenvBase attrs
    // {
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
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [
      "--cfg"
      "tracing_unstable"
    ];
  } // staticPkgConfig;

  # devenv-snix-backend needs protobuf
  devenv-snix-backend = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ protobuf ];
    preConfigure = (attrs.preConfigure or "") + protoSetup;
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [
      "--cfg"
      "tracing_unstable"
    ];
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
  devenv-reload = tracingUnstable;
  devenv-shell = tracingUnstable;
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

  # pest_consume's parser macro generates the AliasedRule enum and dispatch arms
  # by iterating a std::HashMap, so the generated parser (and thus the crate's
  # metadata SVH) is non-deterministic.
  # Patch the macro to sort the rule iteration so its output is reproducible.
  pest_consume_macros = attrs: {
    patches = (attrs.patches or [ ]) ++ [ ./pest-consume-macros-deterministic.patch ];
  };

  # rmcp uses env!("CARGO_CRATE_NAME") at compile time
  rmcp = attrs: {
    CARGO_CRATE_NAME = "rmcp";
  };

  # google-cloud client crates use env!("CARGO_CRATE_NAME") via
  # gaxi::client_request_signals! at compile time
  google-cloud-location = attrs: {
    CARGO_CRATE_NAME = "google_cloud_location";
  };
  google-cloud-iam-v1 = attrs: {
    CARGO_CRATE_NAME = "google_cloud_iam_v1";
  };
  google-cloud-secretmanager-v1 = attrs: {
    CARGO_CRATE_NAME = "google_cloud_secretmanager_v1";
  };

  # nix-bindings crates need pkg-config, nix libs, and bindgen
  nix-bindings-bindgen-raw = attrs: {
    buildInputs = (attrs.buildInputs or [ ]) ++ nixLibs;
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [
      pkg-config
      rustPlatform.bindgenHook
    ];
  } // staticPkgConfig;

  # libghostty-vt-sys has a pkg-config feature that finds the pre-built
  # library from the ghostty flake, so just provide pkg-config + the library.
  #
  # [tier2] For the static build, also enable the crate's `link-static`
  # feature. Its build script otherwise defaults to dynamic linking and emits
  # `-lghostty-vt` (the .so, which our static `.dev` output doesn't ship) →
  # "cannot find -lghostty-vt". With `link-static` it probes the
  # `libghostty-vt-static` pkg-config module and links `libghostty-vt.a`.
  libghostty-vt-sys = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkg-config ];
    buildInputs = (attrs.buildInputs or [ ]) ++ [ libghostty-vt.dev ];
  } // lib.optionalAttrs stdenv.hostPlatform.isStatic {
    features = (attrs.features or [ ]) ++ [ "link-static" ];
  };

  nix-bindings-util = nixLibsOverride;
  nix-bindings-store = nixLibsOverride;
  nix-bindings-expr = nixLibsOverride;
  nix-bindings-flake = nixLibsOverride;
  nix-bindings-fetchers = nixLibsOverride;
  nix-cmd = nixLibsOverride;
}
