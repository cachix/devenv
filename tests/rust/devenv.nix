{
  languages.rust.enable = true;
  enterTest = ''
    if ! command -v clang >/dev/null; then
      echo "clang linker driver is not available"
      exit 1
    fi

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

    if ! grep -q -- "-C linker=clang" "$build_log"; then
      echo "cargo build did not pass the clang linker driver to rustc"
      cat "$build_log"
      exit 1
    fi

    if ! grep -q -- "--cfg devenv_custom_cfg" "$build_log"; then
      echo "clang linker driver clobbered the project's [build] rustflags"
      cat "$build_log"
      exit 1
    fi
  '';
}
