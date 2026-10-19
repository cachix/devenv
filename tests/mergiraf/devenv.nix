{ pkgs, ... }:
{
  mergiraf.enable = true;
  packages = [
    pkgs.git
    pkgs.jq
  ];
}
