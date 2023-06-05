{ pkgs, lib, ... }:

{
  packages = lib.optionals pkgs.stdenv.isDarwin (with pkgs.darwin.apple_sdk; [
    frameworks.Security
  ]);

  languages.rust = {
    enable = true;
    # https://devenv.sh/reference/options/#languagesrusttoolchain
    toolchain = {
      channel = "nightly";
      profile = "default";
    };
  };

  pre-commit.hooks = {
    clippy.enable = true;
    rustfmt.enable = true;
  };
}
