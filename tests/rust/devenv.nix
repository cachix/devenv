{
  languages.rust.enable = true;
  languages.rust.mold.enable = false;
  enterTest = ''
    if [ -n "$RUSTFLAGS" ]; then
      echo "RUSTFLAGS is set, but it should not be"
      exit 1
    fi
  '';
}
