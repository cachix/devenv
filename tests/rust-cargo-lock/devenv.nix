{ config, ... }:

# The Cargo helpers work without `languages.rust.enable` or a rust-overlay
# input, so `Cargo.lock` can drive unrelated parts of the configuration.
let
  inherit (config.languages.rust) cargo;
  ambiguous = builtins.tryEval (cargo.lockedVersion ./. "syn");
in
{
  env = {
    # Found at the workspace root, also when starting from a member directory
    # or pointing at the file itself.
    WASM_BINDGEN_FROM_ROOT = cargo.lockedVersion ./. "wasm-bindgen";
    WASM_BINDGEN_FROM_MEMBER = cargo.lockedVersion ./crates/web "wasm-bindgen";
    WASM_BINDGEN_FROM_FILE = cargo.lockedVersion ./Cargo.lock "wasm-bindgen";
    # Workspace members are locked too.
    WEB_VERSION = cargo.lockedVersion ./. "web";
    LOCKED_PACKAGES = toString (builtins.length (cargo.readLock ./crates/web).package);
    # A crate locked at several versions must be disambiguated by the caller.
    SYN_AMBIGUOUS = if ambiguous.success then "no" else "yes";
  };

  enterTest = ''
    test "$WASM_BINDGEN_FROM_ROOT" = "0.2.100"
    test "$WASM_BINDGEN_FROM_MEMBER" = "0.2.100"
    test "$WASM_BINDGEN_FROM_FILE" = "0.2.100"
    test "$WEB_VERSION" = "0.1.0"
    test "$LOCKED_PACKAGES" = "6"
    test "$SYN_AMBIGUOUS" = "yes"
  '';
}
