{
  languages.vhdl = {
    enable = true;
    compiler = "nvc";
    lsp.enable = false;
  };
  enterTest = ''
    if ! command -v nvc >/dev/null; then
      echo "nvc is not available"
      exit 1
    fi
  '';
}
