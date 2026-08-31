{ ... }:
{
  languages.rust = {
    enable = true;
    lld.enable = true;
    rustdocflags = "--cfg devenv_custom_rustdoc_cfg";
  };

  enterTest = ''
    expected_rustdocflags="-C link-arg=-fuse-ld=lld --cfg devenv_custom_rustdoc_cfg"
    if [ "$RUSTDOCFLAGS" != "$expected_rustdocflags" ]; then
      echo "unexpected RUSTDOCFLAGS: $RUSTDOCFLAGS"
      exit 1
    fi

    workdir=$(mktemp -d)
    cargo init --lib "$workdir/rustdocflags-check" >/dev/null
    cat > "$workdir/rustdocflags-check/src/lib.rs" <<'EOF'
    #[cfg(not(devenv_custom_rustdoc_cfg))]
    compile_error!("languages.rust.rustdocflags was not passed to rustdoc");
    EOF
    ( cd "$workdir/rustdocflags-check" && cargo doc --no-deps )
  '';
}
