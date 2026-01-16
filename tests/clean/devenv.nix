{
  enterTest = ''
    if [ -z "$RUST_LOG" ]; then
      echo "RUST_LOG is not set"
      exit 1
    fi

    set +u
    if [ ! -z "$DATABASE_URL" ]; then
      echo "DATABASE_URL is set"
      exit 1
    fi
  '';
}
