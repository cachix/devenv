{
  languages.rocq.enable = true;

  enterTest = ''
    if ! command -v rocq >/dev/null; then
      echo "ERROR: rocq not found"
      exit 1
    fi

    rocq compile hello.v
  '';
}
