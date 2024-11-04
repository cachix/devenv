{ pkgs, lib, ... }:

{
  languages.rust = {
    enable = true;
    # https://devenv.sh/reference/options/#languagesrustchannel
    channel = "stable";

    targets = [ "wasm32-unknown-unknown" ];

    components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" "rust-std" ];
  };

  git-hooks.hooks = {
    clippy = {
      enable = true;
      settings.offline = false;
      extraPackages = [ pkgs.openssl ];
    };
    rustfmt.enable = true;
  };
  git-hooks.settings.rust.cargoManifestPath = "./Cargo.toml";

  packages = [
    pkgs.wasm-pack
    pkgs.binaryen # use a newer version of wasm-opt
    pkgs.nodejs
  ] ++ lib.optionals pkgs.stdenv.isDarwin (with pkgs.darwin.apple_sdk; [
    frameworks.Security
  ]);
}
