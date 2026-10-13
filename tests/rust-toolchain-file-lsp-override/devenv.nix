{ pkgs, ... }:
let
  analyzer = pkgs.writeShellScriptBin "rust-analyzer" ''
    echo explicit-analyzer-selected
  '';
in
{
  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
    lsp.package = analyzer;
  };

  enterTest = ''
    test "$(command -v rust-analyzer)" = "${analyzer}/bin/rust-analyzer"
    test "$(rust-analyzer --version)" = "explicit-analyzer-selected"
  '';
}
