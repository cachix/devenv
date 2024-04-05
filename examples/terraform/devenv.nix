{ pkgs, ... }:

{
  # For versions >= 1.6.0 open a shell by running the following command:
  #
  # env NIXPKGS_ALLOW_UNFREE=1 devenv --impure shell
  languages.terraform = {
    enable = true;
    version = "1.5.6";
  };
}
