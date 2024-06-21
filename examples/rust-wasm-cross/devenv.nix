{ pkgs, lib, ... }:

{
  languages.rust = {
    enable = true;
    # https://devenv.sh/reference/options/#languagesrustchannel
    channel = "nightly";

    targets = [ "wasm32-unknown-unknown" ];

    components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" "rust-std" ];
  };

  pre-commit.hooks = {
    clippy = {
      enable = true;
      settings.offline = false;
    };
    rustfmt.enable = true;
  };

  packages = [
    pkgs.wasm-pack
    pkgs.binaryen # use a newer version of wasm-opt
    pkgs.nodejs
  ] ++ lib.optionals pkgs.stdenv.isDarwin (with pkgs.darwin.apple_sdk; [
    frameworks.Security
  ]);

  # macOS workaround:
  # The linker on macOS doesn't like the frameworks option when compiling to wasm32.
  # See https://github.com/rust-lang/rust/issues/122333
  env.RUSTFLAGS = lib.mkIf pkgs.stdenv.isDarwin (lib.mkForce "");
}
