{ pkgs, lib, config, ... }:

let
  # Test the crate2nix import functionality
  myapp = config.languages.rust.import ./app { };
in
{
  languages.rust = {
    enable = true;
    # https://devenv.sh/reference/options/#languagesrustchannel
    channel = "nightly";

    components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" ];
  };

  # Include the imported package in the environment
  packages = [ myapp ];

  # Expose the package as an output for testing
  outputs = {
    inherit myapp;
  };

  git-hooks.hooks = {
    rustfmt.enable = true;
    clippy.enable = true;
  };
}
