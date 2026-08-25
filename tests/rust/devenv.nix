{
  config,
  lib,
  pkgs,
  ...
}:
let
  # The clang linker driver is only enabled by default on Linux, so the
  # clang-specific assertions below only apply where the module turns it on.
  clangLinker = config.languages.rust.clangLinker.enable;

  # This is intentionally independent of the module's implementation so a
  # regression in its platform condition cannot silently disable the assertion.
  expectLld = clangLinker && pkgs.stdenv.hostPlatform.rust.rustcTarget == "x86_64-unknown-linux-gnu";

  # The exact linker value the module configured, asserted against the build log.
  rustLinker =
    config.env."CARGO_TARGET_${pkgs.stdenv.hostPlatform.rust.cargoEnvVarTarget}_LINKER" or "";

  # Ask the linker to print its version during the regular build so the LLD
  # assertion below does not need a second compile.
  rustflags = [
    "--cfg"
    "devenv_custom_cfg"
  ]
  ++ lib.optionals expectLld [
    "-C"
    "link-arg=-Wl,-v"
  ];
in
{
  languages.rust.enable = true;
  enterTest = ''
    ${lib.optionalString clangLinker ''
      if ! command -v clang >/dev/null; then
        echo "clang linker driver is not available"
        exit 1
      fi
    ''}

    workdir=$(mktemp -d)
    cargo new --bin "$workdir/linker-check" >/dev/null

    # A project's `.cargo/config.toml` `[build] rustflags` must survive: the clang
    # linker driver is configured via CARGO_TARGET_<triple>_LINKER, not RUSTFLAGS,
    # so it no longer clobbers these flags (RUSTFLAGS would override them entirely).
    mkdir -p "$workdir/linker-check/.cargo"
    printf '[build]\nrustflags = %s\n' '${builtins.toJSON rustflags}' \
      > "$workdir/linker-check/.cargo/config.toml"

    # cargo discovers `.cargo/config.toml` from the working directory, so build
    # from inside the project (in a subshell to keep the test's cwd unchanged).
    build_log="$workdir/build.log"
    ( cd "$workdir/linker-check" && cargo build -vv ) >"$build_log" 2>&1

    ${lib.optionalString clangLinker ''
      if ! grep -qF -- "-C linker=${rustLinker}" "$build_log"; then
        echo "cargo build did not pass the clang linker driver to rustc"
        cat "$build_log"
        exit 1
      fi
    ''}

    ${lib.optionalString expectLld ''
      if ! grep -qF -- "linker stdout: LLD" "$build_log"; then
        echo "the clang linker driver did not use LLD"
        cat "$build_log"
        exit 1
      fi
    ''}

    if ! grep -q -- "--cfg devenv_custom_cfg" "$build_log"; then
      echo "the project's [build] rustflags were clobbered"
      cat "$build_log"
      exit 1
    fi
  '';
}
