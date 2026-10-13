{ config, pkgs, ... }:
{
  languages.rust = {
    enable = true;
    lsp.enable = false;
    toolchainPackage = pkgs.rust-bin.stable."1.89.0".minimal.override {
      extensions = [ ];
    };
  };

  enterTest = ''
    test -z "''${RUST_SRC_PATH+x}"
    sysroot=$(rustc --print sysroot)
    test "$sysroot" = "${config.languages.rust.toolchainPackage}"
    test ! -e "$sysroot/lib/rustlib/src/rust/library/core/Cargo.toml"
  '';
}
