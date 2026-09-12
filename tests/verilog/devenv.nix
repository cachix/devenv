{
  languages.verilog.enable = true;

  enterTest = ''
    if ! command -v verilator >/dev/null; then
      echo "ERROR: verilator not found"
      exit 1
    fi

    if ! command -v verible-verilog-ls >/dev/null; then
      echo "ERROR: verible-verilog-ls not found"
      exit 1
    fi
  '';
}
