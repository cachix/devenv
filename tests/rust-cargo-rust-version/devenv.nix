{ config, ... }:

{
  languages.rust = {
    enable = true;
    channel = "stable";
    version = config.languages.rust.cargoRustVersion ./.;
  };

  enterTest = ''
    rustc --version | grep -F "rustc 1.80.0"
    cargo build
  '';
}
