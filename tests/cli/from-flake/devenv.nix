{ pkgs, inputs, ... }: {
  packages = [
    (pkgs.writeShellScriptBin "from-flake-input" "cat ${inputs.marker}/id.txt")
  ];
}
