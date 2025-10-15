{ mkShell }:
{ projectRoot }:
{
  nixpkgs,
  configuration,
}:
mkShell {
  inputs = {
    inherit nixpkgs;
    self = projectRoot;
  };
  pkgs = nixpkgs;
  modules = [ configuration ];
}
