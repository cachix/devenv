{
  languages.ada.enable = true;

  enterTest = ''
    if ! command -v gnat >/dev/null; then
      echo "gnat is not available"
      exit 1
    fi
  '';
}
