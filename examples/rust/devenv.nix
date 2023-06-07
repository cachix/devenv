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
    sha256 = "sha256-6Q90iheJHltM1tvGGhjMGbRKkJtdTEJ6RTFOUoHxrjg=";
  };

  pre-commit.hooks = {
    clippy.enable = true;
    rustfmt.enable = true;
  };
}
