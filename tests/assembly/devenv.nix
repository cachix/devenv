{
  languages.assembly.enable = true;
  enterTest = ''
    if ! command -v nasm >/dev/null; then
      echo "nasm is not available"
      exit 1
    fi
  '';
}
