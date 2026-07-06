{
  languages.rust.enable = true;
  # TODO: what are we testing here? the mold feature?
  languages.rust.mold.enable = false;
  enterTest = ''
    case "$RUSTFLAGS" in
      *"-C linker=clang"*)
        ;;
      *)
        echo "RUSTFLAGS should configure clang as the linker driver"
        echo "RUSTFLAGS=$RUSTFLAGS"
        exit 1
        ;;
    esac

    if ! command -v clang >/dev/null; then
      echo "clang linker driver is not available"
      exit 1
    fi

    workdir=$(mktemp -d)
    cargo new --bin "$workdir/linker-check" >/dev/null
    build_log="$workdir/build.log"
    cargo build --manifest-path "$workdir/linker-check/Cargo.toml" -vv >"$build_log" 2>&1
    if ! grep -q -- "-C linker=clang" "$build_log"; then
      echo "cargo build did not pass the clang linker driver to rustc"
      cat "$build_log"
      exit 1
    fi
  '';
}
