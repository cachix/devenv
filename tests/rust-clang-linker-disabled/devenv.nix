{
  languages.rust = {
    enable = true;
    clangLinker.enable = false;
  };

  enterTest = ''
    if [ -n "''${RUSTFLAGS+x}" ]; then
      echo "RUSTFLAGS is set, but it should not be"
      echo "RUSTFLAGS=$RUSTFLAGS"
      exit 1
    fi
  '';
}
