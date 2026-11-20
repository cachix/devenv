{
  languages.assembly = {
    enable = true;
    lsp.config.instructionSet = "x86/x86-64";
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

    nasm -f elf64 test_hello_world.asm -o test_hello_world.o
    ld test_hello_world.o -o test_hello_world
    ./test_hello_world
  '';
}
