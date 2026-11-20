{
  languages.d.enable = true;
  enterTest = ''
    if ! command -v ldc2 >/dev/null 2>&1 && ! command -v dub >/dev/null 2>&1; then
      echo "ldc or dub are falling."
      exit 1
    fi

    ldc2 -of=hello hello.d
    ./hello

    echo "D tests passed"
  '';
}
