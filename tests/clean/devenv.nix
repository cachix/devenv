{
  enterTest = ''
    if [ -z "$DATABASE_URL" ]; then
      echo "DATABASE_URL is not set"
      exit 1
    fi

    set +u
    if [ ! -z "$BROWSER" ]; then
      echo "BROWSER is set"
      exit 1
    fi
  '';
}
