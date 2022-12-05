{ pkgs, ... }:

{
  languages.rust = {
    enable = true;
    version = "latest";
  };

  pre-commit.hooks = {
    clippy.enable = true;
    rustfmt.enable = true;
  };
}
