{
  enterTest = ''
    if [ -z "$DEVENV_NIX" ]; then
      echo "DEVENV_NIX is not set"
      exit 1
    fi

    set +u
    if [ ! -z "$BROWSER" ]; then
      echo "BROWSER is set"
      exit 1
    fi
  '';
}
