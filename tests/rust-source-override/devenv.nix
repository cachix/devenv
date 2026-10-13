{ pkgs, ... }:
let
  # A distinct output path ensures this really overrides the module default.
  sources = pkgs.runCommand "rust-sources-override" { } ''
    ln -s ${pkgs.rustPlatform.rustLibSrc} "$out"
  '';
in
{
  languages.rust.enable = true;
  languages.rust.lsp.enable = false;
  env.RUST_SRC_PATH = sources;

  enterTest = ''
    test "$RUST_SRC_PATH" = "${sources}"
    test -f "$RUST_SRC_PATH/core/Cargo.toml"
  '';
}
