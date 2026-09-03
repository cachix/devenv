{
  languages.assembly = {
    enable = true;
    type = "riscv"; # assumes riscv-64 bits by default.
  };

  enterTest = ''
    if ! command -v asm-lsp >/dev/null; then
      echo "ERROR: asm-lsp not found"
      exit 1
    fi

    if ! command -v cc >/dev/null; then
      echo "ERROR: cc not found"
      exit 1
    fi

    if ! test -f .asm-lsp.toml; then
      echo "ERROR: .asm-lsp.toml not found."
      echo "> This shouldn't happen."
      exit 1
    fi
  '';
}
