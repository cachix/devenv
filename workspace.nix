# Tooling to build the workspace crates using crate2nix
{ pkgs
, lib
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
, gitMinimal
, rustPlatform
, buildRustCrate
, defaultCrateOverrides
, rustc
, cargo
, cargoProfile ? "release"
, gitRev ? ""
, isRelease ? false
}:

let
  cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
  version = cargoToml.workspace.package.version;

  # Override buildRustCrate to use the newer rustc from languages.rust
  # The default buildRustCrate uses an older rustc (1.73) which doesn't support
  # APIs needed by newer crate versions like clap_lex 0.7.6
  buildRustCrateNew = buildRustCrate.override {
    inherit rustc cargo;
  };

  # Import crate2nix generated file with overrides
  crateConfig = import ./crate-config.nix {
    inherit
      lib
      stdenv
      nix
      openssl
      dbus
      protobuf
      pkg-config
      llvmPackages
      boehmgc
      cachix
      nixd
      makeBinaryWrapper
      installShellFiles
      glibcLocalesUtf8
      rustPlatform
      gitRev
      ;
  };

  cargoNix = import ./Cargo.nix {
    inherit pkgs lib stdenv;
    buildRustCrateForPkgs = _: buildRustCrateNew;
    defaultCrateOverrides = defaultCrateOverrides // crateConfig;
    release = cargoProfile == "release" || cargoProfile == "release_fast";
    # Enable tracing_unstable for dependency resolution so valuable crate is included.
    # This matches the --cfg tracing_unstable passed via crate-config.nix at compile time.
    extraTargetFlags = { tracing_unstable = true; };
  };

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
      '';
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

    # Fast build variant (same as regular since crate2nix doesn't support profiles)
    devenv-tasks-fast-build = cargoNix.workspaceMembers.devenv-tasks.build;
  };
}
