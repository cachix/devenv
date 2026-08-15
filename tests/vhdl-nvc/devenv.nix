{
  languages.vhdl = {
    enable = true;
    compiler = "nvc";
    lsp.enable = false;
  };
  enterTest = ''
    set -euo pipefail

    if ! command -v nvc >/dev/null; then
      echo "nvc is not available"
      exit 1
    fi

    workdir="$(mktemp -d)"
    trap 'rm -rf "$workdir"' EXIT

    cp adder.vhd "$workdir/adder.vhd"
    cp tb_adder.vhd "$workdir/tb_adder.vhd"

    cd "$workdir"

    nvc --version
    nvc -a adder.vhd tb_adder.vhd
    nvc -e tb_adder
    nvc -r tb_adder
  '';
}
