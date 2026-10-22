{
  languages.vhdl = {
    enable = true;
    lsp.enable = false;
  };
  enterTest = ''
    if ! command -v ghdl >/dev/null; then
      echo "ghdl is not available"
      exit 1
    fi
  '';
}
