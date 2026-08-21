{
  languages.vhdl = {
    enable = true;
    lsp.enable = false;
  };
  enterTest = ''
    set -euo pipefail

    if ! command -v ghdl >/dev/null; then
      echo "ghdl is not available"
      exit 1
    fi

    workdir="$(mktemp -d)"
    trap 'rm -rf "$workdir"' EXIT

    cp adder.vhd "$workdir/adder.vhd"
    cp tb_adder.vhd "$workdir/tb_adder.vhd"

    cd "$workdir"

    ghdl -a --std=08 adder.vhd tb_adder.vhd
    ghdl -e --std=08 tb_adder
    ghdl -r --std=08 tb_adder --assert-level=error
  '';
}
