{ pkgs, config, ... }:

let
  # wasm-bindgen-cli must match the wasm-bindgen crate version exactly.
  wasmBindgenVersion = config.languages.rust.cargo.lockedVersion ./. "wasm-bindgen";
  wasmBindgenCli = pkgs.wasm-bindgen-cli_0_2_100;
in
{
  languages.rust = {
    enable = true;
    # https://devenv.sh/reference/options/#languagesrustchannel
    channel = "stable";

    targets = [ "wasm32-unknown-unknown" ];

    components = [
      "rustc"
      "cargo"
      "clippy"
      "rustfmt"
      "rust-analyzer"
      "rust-std"
    ];
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
    wasmBindgenCli
    pkgs.binaryen # use a newer version of wasm-opt
    pkgs.nodejs
  ];

  assertions = [
    {
      assertion = wasmBindgenCli.version == wasmBindgenVersion;
      message = "wasm-bindgen-cli ${wasmBindgenCli.version} does not match wasm-bindgen ${wasmBindgenVersion} in Cargo.lock";
    }
  ];
}
