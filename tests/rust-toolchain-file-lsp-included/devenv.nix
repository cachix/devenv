{ config, pkgs, ... }:
let
  expectedAnalyzer = config.languages.rust.toolchainPackage;
in
{
  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
  };

  enterTest = ''
    test "${config.languages.rust.lsp.package}" = "${expectedAnalyzer}"
    test "$(command -v rust-analyzer)" = "${expectedAnalyzer}/bin/rust-analyzer"
    rust-analyzer --version
  '';
}
