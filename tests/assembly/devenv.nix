{ pkgs, ... }: {
  languages.assembly = {
    enable = true;
    type = "riscv"; # assumes riscv-64 bits by default.
  };

  enterTest = ''
    echo "Checking asm-lsp"
    if ! command -v asm-lsp >/dev/null; then
      echo "ERROR: asm-lsp not found"
      exit 1
    fi
    echo "asm-lsp v$(asm-lsp version) is Ok ✓"

    echo "Checking RISC-V compiler"
    if ! command -v cc >/dev/null; then
      echo "ERROR: cc not found"
      exit 1
    fi
    echo "$(cc --version | grep "gcc") is Ok ✓"

    echo "Checking generated .asm-lsp.toml"
    if ! test -f .asm-lsp.toml; then
      echo "ERROR: .asm-lsp.toml not found."
      echo "> This shouldn't happen."
      exit 1
    fi

    echo "Assembly integration Ok ✓"
  '';
}
