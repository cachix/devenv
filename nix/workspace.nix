# Tooling to build the workspace crates using crate2nix
{ lib
, stdenv
, buildPackages

, openssl
, dbus
, protobuf
, pkg-config
, llvmPackages
, cachix
, libghostty-vt ? null
, nix
, nixd

  # Helpers
, callPackage
, makeBinaryWrapper
, installShellFiles
, glibcLocalesUtf8

  # Rust
, defaultCrateOverrides
, rustc
, cargo
, cargoProfile ? "release"

  # [tier2] Static (musl) build: force non-PIE codegen on every crate AND its
  # build.rs, since rust+musl defaults to static-PIE which segfaults on exec.
, buildStatic ? false

, gitRev ? ""
, isRelease ? false
}:

let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
  inherit (cargoToml.workspace.package) version;

  # Import crate2nix generated file with overrides
  crateConfig = callPackage ./crate-config.nix { inherit gitRev isRelease libghostty-vt; };

  # rust+musl defaults to static-PIE, which segfaults on exec (the musl-gcc
  # wrappers don't support static-PIE — rust-lang/rust#95926). Fix: non-PIE
  # codegen via -Crelocation-model=static plus -Clink-arg=-no-pie to force a
  # non-PIE link, so the final musl host binary actually runs. These apply to
  # the host (musl) crate compiles only.
  staticRustcOpts = [ "-Crelocation-model=static" "-Clink-arg=-no-pie" ];

  # Build scripts (build.rs) are the real Tier 2 blocker. crate2nix compiles a
  # crate's build.rs *inside the musl host derivation*, but with NO `--target`
  # and NO `-C linker`. So rustc falls back to its default target — which, for
  # the rust-overlay toolchain, is x86_64-unknown-linux-GNU — and its default
  # linker, which is `cc` on PATH = the musl-static wrapper inside the host
  # derivation. gnu-target objects linked by the musl cc segfault the instant
  # the build runs them (exit 139, first hit at anyhow). The build-dependency
  # rlibs build.rs links come from `pkgs.buildPackages` and are gnu-target, so
  # the gnu default is correct — the link is what's wrong. Two fixes, together:
  #
  #   1. Point build.rs at the glibc build-host cc (`-C linker=`), so gnu
  #      objects are linked by a gnu linker.
  #   2. Force the glibc dynamic linker onto the build.rs link explicitly. The
  #      musl-static stdenv sets `NIX_CFLAGS_LINK=-static` globally, which the
  #      build cc's setup hook merges into its gnu role. With `-static` present
  #      the cc wrapper treats the link as static and skips `-dynamic-linker` —
  #      but rust still emits dynamic `-l` flags, so the result is a dynamic ELF
  #      with DT_NEEDED libc.so.6 yet NO PT_INTERP: unloadable, segfaults on
  #      exec. Passing `-Wl,-dynamic-linker,<glibc ld.so>` ourselves bypasses
  #      that guard and restores the interpreter, so build.rs runs.
  buildCc = "${buildPackages.stdenv.cc}/bin/${buildPackages.stdenv.cc.targetPrefix}cc";
  buildRsRustcOpts = [
    "-C"
    "linker=${buildCc}"
    "-C"
    "link-arg=-Wl,-dynamic-linker,${buildPackages.stdenv.cc.bintools.dynamicLinker}"
  ];

  injectStatic = crate: crate // {
    extraRustcOpts = (crate.extraRustcOpts or [ ]) ++ staticRustcOpts;
    extraRustcOptsForBuildRs = (crate.extraRustcOptsForBuildRs or [ ]) ++ buildRsRustcOpts;
  };

  # Override the rust toolchain and bake the per-crate overrides into the
  # builder. We pass `defaultCrateOverrides` through unchanged to crate2nix so it
  # uses this builder directly rather than calling `.override` on it (which would
  # strip the static wrapper, since a plain function has no `.override`).
  #
  # crate2nix calls this for two package sets: the musl *host* set (isStatic)
  # for crates that end up in the binary, and `pkgs.buildPackages` (glibc) for
  # build-time deps (proc-macros, build scripts). The static codegen flags must
  # apply ONLY to the musl host crates. Applying `-Crelocation-model=static` to
  # the glibc build-platform rlibs makes them non-PIC, then the default-PIE
  # build.rs executables that link them fail with `R_X86_64_32 … recompile with
  # -fPIC` (first hit: indexmap linking autocfg). Gate on `isStatic`.
  buildRustCrateForPkgs =
    pkgs:
    let
      base = pkgs.buildRustCrate.override {
        inherit rustc cargo;
        defaultCrateOverrides = defaultCrateOverrides // crateConfig;
      };
    in
    if buildStatic && pkgs.stdenv.hostPlatform.isStatic then
      (crate: base (injectStatic crate))
    else
      base;

  cargoNix = callPackage ../Cargo.nix {
    inherit buildRustCrateForPkgs;
    inherit defaultCrateOverrides;
    release = cargoProfile == "release";
    # Enable tracing_unstable for dependency resolution so valuable crate is included.
    # This matches the --cfg tracing_unstable passed via crate-config.nix at compile time.
    extraTargetFlags = { tracing_unstable = true; };
  };

  xtask = cargoNix.workspaceMembers.xtask.build;

  # Wrap the devenv binary with required paths
  wrapDevenv = drv: stdenv.mkDerivation {
    pname = "devenv-wrapped";
    inherit version;
    src = drv;

    nativeBuildInputs = [ makeBinaryWrapper installShellFiles ];

    # Include devenv-run-tests in the output
    devenvRunTests = cargoNix.workspaceMembers.devenv-run-tests.build;

    installPhase =
      let
        setDefaultLocaleArchive = lib.optionalString (glibcLocalesUtf8 != null) ''
          --set-default LOCALE_ARCHIVE ${glibcLocalesUtf8}/lib/locale/locale-archive
        '';
      in
      ''
        mkdir -p $out/bin

        cp $src/bin/devenv $out/bin/
        cp $devenvRunTests/bin/devenv-run-tests $out/bin/

        wrapProgram $out/bin/devenv \
          --prefix PATH ":" "$out/bin:${lib.getBin cachix}/bin:${lib.getBin nixd}/bin" \
          ${setDefaultLocaleArchive}

        wrapProgram $out/bin/devenv-run-tests \
          --prefix PATH ":" "$out/bin:${lib.getBin cachix}/bin:${lib.getBin nixd}/bin" \
          ${setDefaultLocaleArchive}

        # Generate manpages
        ${xtask}/bin/xtask generate-manpages --out-dir man
        installManPage man/*

        # Generate shell completions (devenv must be in PATH)
        compdir=./completions
        export PATH="$out/bin:$PATH"
        for shell in bash fish zsh; do
          ${xtask}/bin/xtask generate-shell-completion $shell --out-dir $compdir
        done

        installShellCompletion --cmd devenv \
          --bash $compdir/devenv.bash \
          --fish $compdir/devenv.fish \
          --zsh $compdir/_devenv
      '';

    meta.mainProgram = "devenv";
  };
in
{
  inherit version;

  # Expose raw crate builds for debugging/development
  rawCrates = cargoNix.workspaceMembers;

  crates = {
    # Main devenv package with wrapping and shell completions
    devenv = wrapDevenv cargoNix.workspaceMembers.devenv.build;

    # devenv-tasks standalone
    devenv-tasks = cargoNix.workspaceMembers.devenv-tasks.build;
  };
}
