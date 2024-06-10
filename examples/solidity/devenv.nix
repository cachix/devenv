{ pkgs, lib, ... }:
{
  languages.solidity = {
    enable = true;
    foundry.enable = true;
  };
}
