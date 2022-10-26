{ pkgs, ... }: 

{
  packages = [ (import ./pkg.nix pkgs) ];
  a
}