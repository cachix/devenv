{
  languages.rust.enable = true;
  # TODO: what are we testing here? the mold feature?
  languages.rust.mold.enable = false;
  enterTest = ''
    if [ -n "''${RUSTFLAGS+x}" ]; then
      echo "RUSTFLAGS is set, but it should not be"
      exit 1
    fi
  '';
}
