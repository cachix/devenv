{ lib, ... }:

{
  pre-commit.hooks.statix.enable = lib.mkForce false;
}
