{ config, lib, ... }:
let
  # The clang linker driver is only enabled by default on Linux, so the
  # clang-specific assertions below only apply where the module turns it on.
  clangLinker = config.languages.rust.clangLinker.enable;
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
    printf '[build]\nrustflags = ["--cfg", "devenv_custom_cfg"]\n' \
      > "$workdir/linker-check/.cargo/config.toml"

    # cargo discovers `.cargo/config.toml` from the working directory, so build
    # from inside the project (in a subshell to keep the test's cwd unchanged).
    build_log="$workdir/build.log"
    ( cd "$workdir/linker-check" && cargo build -vv ) >"$build_log" 2>&1

    ${lib.optionalString clangLinker ''
      if ! grep -q -- "-C linker=clang" "$build_log"; then
        echo "cargo build did not pass the clang linker driver to rustc"
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
