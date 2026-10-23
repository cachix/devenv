{ config, ... }:

let
  inherit (config.languages.rust) cargo;
in
{
  languages.rust = {
    enable = true;
    channel = "stable";
    # `crates/app` inherits `rust-version` from the workspace root: this
    # resolves it by walking up to the `[workspace]` manifest.
    version = cargo.rustVersion ./crates/app;
  };

  env = {
    # The same value read from the workspace root and from the manifest file.
    MSRV_FROM_ROOT = cargo.rustVersion ./.;
    MSRV_FROM_FILE = cargo.rustVersion ./crates/app/Cargo.toml;
    APP_VERSION = (cargo.readManifest ./crates/app).package.version;
  };

  enterTest = ''
    rustc --version | grep -F "rustc 1.80.0"
    test "$MSRV_FROM_ROOT" = "1.80.0"
    test "$MSRV_FROM_FILE" = "1.80.0"
    test "$APP_VERSION" = "0.3.1"
    cargo build
  '';
}
